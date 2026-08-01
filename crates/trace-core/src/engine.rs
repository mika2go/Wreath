use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{Codec, Config};
use crate::display::Monitor;
use crate::paths::AppPaths;

const SAVE_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaySpec {
    pub monitor: String,
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u16,
    pub duration_seconds: u16,
    pub codec: Codec,
    pub quality: u8,
    pub cursor: bool,
    pub desktop_audio: bool,
    pub microphone_audio: bool,
    pub microphone_device: Option<String>,
    pub microphone_gain_percent: u16,
    pub output_directory: PathBuf,
    pub portal_session_token_file: Option<PathBuf>,
}

#[derive(Debug)]
pub enum EngineError {
    Io(io::Error),
    Exited(Option<i32>),
    Signal(String),
    SaveTimeout,
    OutputClosed,
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) if error.kind() == io::ErrorKind::NotFound => write!(
                formatter,
                "gpu-screen-recorder is not installed; on Arch or CachyOS run \
                 `sudo pacman -S gpu-screen-recorder`"
            ),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Exited(code) => write!(
                formatter,
                "gpu-screen-recorder exited unexpectedly{}",
                code.map(|value| format!(" with code {value}"))
                    .unwrap_or_default()
            ),
            Self::Signal(message) => write!(formatter, "cannot control recorder: {message}"),
            Self::SaveTimeout => write!(formatter, "timed out while saving replay"),
            Self::OutputClosed => write!(formatter, "recorder output closed before clip was saved"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<io::Error> for EngineError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl ReplaySpec {
    pub fn from_config(config: &Config, monitor: &Monitor) -> Self {
        Self {
            monitor: monitor.name.clone(),
            width: monitor.width,
            height: monitor.height,
            frames_per_second: config.capture.frames_per_second,
            duration_seconds: config.capture.duration_seconds,
            codec: config.capture.codec,
            quality: config.capture.quality,
            cursor: config.capture.cursor,
            desktop_audio: config.audio.desktop,
            microphone_audio: config.audio.microphone,
            microphone_device: config.audio.microphone_device.clone(),
            microphone_gain_percent: config.audio.microphone_gain_percent,
            output_directory: config.storage.directory.clone(),
            portal_session_token_file: monitor
                .uses_portal()
                .then(|| AppPaths::discover().cache_dir.join("portal-session-token")),
        }
    }

    pub fn arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            "-w".into(),
            self.monitor.clone().into(),
            "-f".into(),
            self.frames_per_second.to_string().into(),
            "-r".into(),
            self.duration_seconds.to_string().into(),
            "-c".into(),
            "mp4".into(),
            "-bm".into(),
            "cbr".into(),
            "-q".into(),
            self.target_bitrate_kbps().to_string().into(),
            "-df".into(),
            "no".into(),
            "-fm".into(),
            "vfr".into(),
            "-tune".into(),
            "performance".into(),
            "-fallback-cpu-encoding".into(),
            "no".into(),
            "-cursor".into(),
            if self.cursor { "yes" } else { "no" }.into(),
            "-o".into(),
            self.output_directory.as_os_str().to_owned(),
        ];
        match self.codec {
            Codec::Auto => {}
            Codec::H264 => arguments.extend(["-k".into(), "h264".into()]),
            Codec::Hevc => arguments.extend(["-k".into(), "hevc".into()]),
            Codec::Av1 => arguments.extend(["-k".into(), "av1".into()]),
        }
        let microphone = self
            .microphone_audio
            .then(|| self.microphone_device.as_deref().unwrap_or("default_input"));
        let audio_source = match (self.desktop_audio, microphone) {
            (true, Some(microphone)) => Some(format!("default_output|{microphone}")),
            (true, None) => Some("default_output".into()),
            (false, Some(microphone)) => Some(microphone.into()),
            (false, None) => None,
        };
        if let Some(audio_source) = audio_source {
            arguments.extend(["-a".into(), audio_source.into(), "-ac".into(), "aac".into()]);
        }
        if let Some(token_file) = &self.portal_session_token_file {
            arguments.extend([
                "-restore-portal-session".into(),
                "yes".into(),
                "-portal-session-token-filepath".into(),
                token_file.as_os_str().to_owned(),
            ]);
        }
        arguments
    }

    pub fn target_bitrate_kbps(&self) -> u32 {
        let pixels_per_second =
            u64::from(self.width) * u64::from(self.height) * u64::from(self.frames_per_second);
        let bits_per_pixel_milli = match self.codec {
            Codec::Auto | Codec::H264 => 160_u64,
            Codec::Hevc => 115,
            Codec::Av1 => 90,
        };
        let quality_factor = 50_u64 + u64::from(self.quality);
        let bitrate = pixels_per_second
            .saturating_mul(bits_per_pixel_milli)
            .saturating_mul(quality_factor)
            / 125_000_000;
        u32::try_from(bitrate.clamp(2_500, 80_000)).unwrap_or(80_000)
    }

    pub fn estimated_buffer_megabytes(&self) -> u64 {
        u64::from(self.target_bitrate_kbps()).saturating_mul(u64::from(self.duration_seconds))
            / 8
            / 1_024
    }
}

pub struct GpuScreenRecorder {
    child: Child,
    saved_paths: Receiver<PathBuf>,
    microphone_gain_source: Option<MicrophoneGainSource>,
}

struct MicrophoneGainSource {
    module_id: String,
    name: String,
}

impl MicrophoneGainSource {
    fn create(master: &str, gain_percent: u16) -> Result<Self, EngineError> {
        Self::unload_stale_sources();
        let name = format!("trace_recording_mic_{}", std::process::id());
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-remap-source",
                &format!("master={master}"),
                &format!("source_name={name}"),
                "source_properties=device.description=TraceRecordingMicrophone",
                "channels=2",
                "channel_map=front-left,front-right",
                "remix=yes",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !output.status.success() {
            return Err(EngineError::Signal(format!(
                "cannot create isolated microphone channel: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let source = Self {
            module_id: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            name,
        };
        let status = Command::new("pactl")
            .args([
                "set-source-volume",
                source.name.as_str(),
                &format!("{gain_percent}%"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(EngineError::Signal(
                "cannot set isolated microphone recording level".into(),
            ));
        }
        Ok(source)
    }

    fn unload_stale_sources() {
        let Ok(output) = Command::new("pactl")
            .args(["list", "short", "modules"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            return;
        };
        for module_id in stale_trace_module_ids(&String::from_utf8_lossy(&output.stdout)) {
            let _ = Command::new("pactl")
                .args(["unload-module", module_id.as_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

impl Drop for MicrophoneGainSource {
    fn drop(&mut self) {
        let _ = Command::new("pactl")
            .args(["unload-module", self.module_id.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn stale_trace_module_ids(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let module_id = fields.next()?;
            let module_name = fields.next()?;
            let arguments = fields.next()?;
            (module_name == "module-remap-source"
                && arguments.contains("source_name=trace_recording_mic_"))
            .then(|| module_id.to_owned())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderCapabilities {
    pub vendor: String,
    pub video_codecs: Vec<String>,
}

impl GpuScreenRecorder {
    pub fn start(spec: &ReplaySpec) -> Result<Self, EngineError> {
        let executable =
            std::env::var_os("TRACE_RECORDER").unwrap_or_else(|| "gpu-screen-recorder".into());
        Self::start_with_executable(spec, executable)
    }

    fn start_with_executable(
        spec: &ReplaySpec,
        executable: impl AsRef<std::ffi::OsStr>,
    ) -> Result<Self, EngineError> {
        fs::create_dir_all(&spec.output_directory)?;
        if let Some(parent) = spec
            .portal_session_token_file
            .as_ref()
            .and_then(|path| path.parent())
        {
            fs::create_dir_all(parent)?;
        }
        let microphone_gain_source = if spec.microphone_audio {
            let master = spec
                .microphone_device
                .as_deref()
                .unwrap_or("@DEFAULT_SOURCE@");
            Some(MicrophoneGainSource::create(
                master,
                spec.microphone_gain_percent,
            )?)
        } else {
            None
        };
        let mut recorder_spec = spec.clone();
        if let Some(source) = &microphone_gain_source {
            recorder_spec.microphone_device = Some(source.name.clone());
        }
        let mut child = Command::new(executable)
            .args(recorder_spec.arguments())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Io(io::Error::other("recorder stdout was not captured")))?;
        let (saved_sender, saved_paths) = mpsc::channel();
        thread::Builder::new()
            .name("trace-save-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        let _ = saved_sender.send(PathBuf::from(trimmed));
                    }
                }
            })?;
        Ok(Self {
            child,
            saved_paths,
            microphone_gain_source,
        })
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    pub fn is_running(&mut self) -> Result<bool, EngineError> {
        match self.child.try_wait()? {
            None => Ok(true),
            Some(status) => Err(EngineError::Exited(status.code())),
        }
    }

    pub fn save(&mut self) -> Result<PathBuf, EngineError> {
        self.is_running()?;
        send_signal(self.process_id(), "USR1")?;
        match self.saved_paths.recv_timeout(SAVE_TIMEOUT) {
            Ok(path) => Ok(path),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::SaveTimeout),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::OutputClosed),
        }
    }

    pub fn stop(&mut self) -> Result<(), EngineError> {
        if self.child.try_wait()?.is_some() {
            let _ = send_signal_to_group(self.process_id(), "KILL");
            self.microphone_gain_source.take();
            return Ok(());
        }
        let _ = send_signal_to_group(self.process_id(), "INT");
        let deadline = Instant::now() + STOP_TIMEOUT;
        while self.child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                if send_signal_to_group(self.process_id(), "KILL").is_err() {
                    self.child.kill()?;
                }
                self.child.wait()?;
                break;
            }
            thread::sleep(STOP_POLL_INTERVAL);
        }
        self.microphone_gain_source.take();
        Ok(())
    }
}

impl Drop for GpuScreenRecorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn send_signal(process_id: u32, signal: &str) -> Result<(), EngineError> {
    let status = Command::new("kill")
        .args(["-s", signal, &process_id.to_string()])
        .status()
        .map_err(EngineError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(EngineError::Signal(format!(
            "signal {signal} for process {process_id} failed"
        )))
    }
}

fn send_signal_to_group(process_id: u32, signal: &str) -> Result<(), EngineError> {
    let process_group = format!("-{process_id}");
    let status = Command::new("kill")
        .args(["-s", signal, "--", &process_group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(EngineError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(EngineError::Signal(format!(
            "signal {signal} for process group {process_id} failed"
        )))
    }
}

pub fn recorder_available() -> bool {
    recorder_capabilities().is_ok()
}

pub fn recorder_capabilities() -> Result<RecorderCapabilities, EngineError> {
    let output = Command::new("gpu-screen-recorder")
        .arg("--info")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(EngineError::Exited(output.status.code()));
    }
    Ok(parse_recorder_info(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_recorder_info(output: &str) -> RecorderCapabilities {
    let mut section = "";
    let mut vendor = "unknown".to_owned();
    let mut video_codecs = Vec::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = line.strip_prefix("section=") {
            section = value;
            continue;
        }
        if section == "gpu_info" {
            if let Some(value) = line.strip_prefix("vendor|") {
                vendor = value.to_owned();
            }
        } else if section == "video_codecs" && !line.contains('|') {
            video_codecs.push(line.to_owned());
        }
    }
    RecorderCapabilities {
        vendor,
        video_codecs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ReplaySpec {
        ReplaySpec {
            monitor: "DP-1".into(),
            width: 1920,
            height: 1080,
            frames_per_second: 60,
            duration_seconds: 30,
            codec: Codec::H264,
            quality: 75,
            cursor: true,
            desktop_audio: true,
            microphone_audio: false,
            microphone_device: None,
            microphone_gain_percent: 100,
            output_directory: PathBuf::from("/tmp/trace-test"),
            portal_session_token_file: None,
        }
    }

    #[test]
    fn command_uses_direct_monitor_and_replay_mode() {
        let arguments = spec()
            .arguments()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(arguments.windows(2).any(|pair| pair == ["-w", "DP-1"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-r", "30"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-k", "h264"]));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-a", "default_output"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-fallback-cpu-encoding", "no"])
        );
        assert!(arguments.windows(2).any(|pair| pair == ["-cursor", "yes"]));
    }

    #[test]
    fn command_mixes_selected_microphone_with_desktop_audio() {
        let mut replay = spec();
        replay.microphone_audio = true;
        replay.microphone_device = Some("alsa_input.usb-shure".into());
        let arguments = replay
            .arguments()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-a", "default_output|alsa_input.usb-shure"])
        );
        assert_eq!(
            arguments
                .iter()
                .filter(|argument| argument.as_str() == "-a")
                .count(),
            1
        );
    }

    #[test]
    fn portal_capture_restores_the_users_session_choice() {
        let mut replay = spec();
        replay.monitor = "portal".into();
        replay.portal_session_token_file = Some(PathBuf::from("/tmp/trace-portal-token"));
        let arguments = replay
            .arguments()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-restore-portal-session", "yes"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-portal-session-token-filepath", "/tmp/trace-portal-token"])
        );
    }

    #[test]
    fn buffer_estimate_is_bounded_and_nonzero() {
        let replay = spec();
        assert!(replay.target_bitrate_kbps() >= 2_500);
        assert!(replay.estimated_buffer_megabytes() > 0);
        assert!(replay.estimated_buffer_megabytes() < 100);
    }

    #[test]
    fn parses_amd_and_nvidia_recorder_capabilities() {
        let amd = parse_recorder_info(
            "section=gpu_info\nvendor|amd\nsection=video_codecs\nh264\nhevc\nav1\n",
        );
        assert_eq!(amd.vendor, "amd");
        assert_eq!(amd.video_codecs, ["h264", "hevc", "av1"]);

        let nvidia = parse_recorder_info(
            "section=gpu_info\nvendor|nvidia\nsection=video_codecs\nh264\nhevc\n",
        );
        assert_eq!(nvidia.vendor, "nvidia");
        assert_eq!(nvidia.video_codecs, ["h264", "hevc"]);
    }

    #[test]
    fn identifies_only_stale_trace_microphone_modules() {
        let modules = concat!(
            "10\tmodule-remap-source\tmaster=mic source_name=trace_recording_mic_123 remix=yes\t\n",
            "11\tmodule-remap-source\tmaster=mic source_name=other_app\t\n",
            "12\tmodule-null-sink\tsink_name=trace_recording_mic_456\t\n",
        );
        assert_eq!(stale_trace_module_ids(modules), ["10"]);
    }

    #[test]
    fn recorder_lifecycle_saves_and_stops() {
        let executable =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fake-recorder.sh");
        let mut recorder = GpuScreenRecorder::start_with_executable(&spec(), executable).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(recorder.is_running().unwrap());
        assert_eq!(
            recorder.save().unwrap(),
            PathBuf::from("/tmp/trace-test/clip.mp4")
        );
        recorder.stop().unwrap();
    }
}
