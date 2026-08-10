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

use crate::config::Codec;
use crate::paths::AppPaths;
use crate::replay::{ReplayBackend, ReplayRecorder, ReplaySpec};

const SAVE_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

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

fn recorder_arguments(
    spec: &ReplaySpec,
    portal_session_token_file: Option<&std::path::Path>,
    desktop_source: Option<&str>,
) -> Vec<OsString> {
    let mut arguments = vec![
        "-w".into(),
        spec.monitor.clone().into(),
        "-f".into(),
        spec.frames_per_second.to_string().into(),
        "-r".into(),
        spec.duration_seconds.to_string().into(),
        "-c".into(),
        "mp4".into(),
        "-bm".into(),
        "cbr".into(),
        "-q".into(),
        spec.target_bitrate_kbps().to_string().into(),
        "-df".into(),
        "no".into(),
        "-fm".into(),
        "vfr".into(),
        "-tune".into(),
        "performance".into(),
        "-fallback-cpu-encoding".into(),
        "no".into(),
        "-cursor".into(),
        if spec.cursor { "yes" } else { "no" }.into(),
        "-o".into(),
        spec.output_directory.as_os_str().to_owned(),
    ];
    match spec.codec {
        Codec::Auto => {}
        Codec::H264 => arguments.extend(["-k".into(), "h264".into()]),
        Codec::Hevc => arguments.extend(["-k".into(), "hevc".into()]),
        Codec::Av1 => arguments.extend(["-k".into(), "av1".into()]),
    }
    let microphone = spec
        .microphone_audio
        .then(|| spec.microphone_device.as_deref().unwrap_or("default_input"));
    let audio_source = match (spec.desktop_audio, microphone) {
        (true, Some(microphone)) => Some(format!(
            "{}|{microphone}",
            desktop_source.unwrap_or("default_output")
        )),
        (true, None) => Some(desktop_source.unwrap_or("default_output").into()),
        (false, Some(microphone)) => Some(microphone.into()),
        (false, None) => None,
    };
    if let Some(audio_source) = audio_source {
        arguments.extend(["-a".into(), audio_source.into(), "-ac".into(), "aac".into()]);
    }
    if let Some(token_file) = portal_session_token_file {
        arguments.extend([
            "-restore-portal-session".into(),
            "yes".into(),
            "-portal-session-token-filepath".into(),
            token_file.as_os_str().to_owned(),
        ]);
    }
    arguments
}

pub struct GpuScreenRecorder {
    child: Child,
    saved_paths: Receiver<PathBuf>,
    microphone_gain_source: Option<RecordingGainSource>,
    desktop_gain_source: Option<RecordingGainSource>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GpuScreenRecorderBackend;

impl ReplayBackend for GpuScreenRecorderBackend {
    type Error = EngineError;
    type Recorder = GpuScreenRecorder;

    fn start(&self, spec: &ReplaySpec) -> Result<Self::Recorder, Self::Error> {
        GpuScreenRecorder::start(spec)
    }
}

struct RecordingGainSource {
    module_id: String,
    name: String,
}

impl RecordingGainSource {
    fn create(master: &str, gain_percent: u16, channel: &str) -> Result<Self, EngineError> {
        let name = format!("wreath_recording_{channel}_{}", std::process::id());
        let description = format!("WreathRecording{channel}");
        let output = Command::new("pactl")
            .args([
                "load-module",
                "module-remap-source",
                &format!("master={master}"),
                &format!("source_name={name}"),
                &format!("source_properties=device.description={description}"),
                "channels=2",
                "channel_map=front-left,front-right",
                "remix=yes",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if !output.status.success() {
            return Err(EngineError::Signal(format!(
                "cannot create isolated {channel} channel: {}",
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
            return Err(EngineError::Signal(format!(
                "cannot set isolated {channel} recording level"
            )));
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
        for module_id in stale_wreath_module_ids(&String::from_utf8_lossy(&output.stdout)) {
            let _ = Command::new("pactl")
                .args(["unload-module", module_id.as_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

impl Drop for RecordingGainSource {
    fn drop(&mut self) {
        let _ = Command::new("pactl")
            .args(["unload-module", self.module_id.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn stale_wreath_module_ids(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let module_id = fields.next()?;
            let module_name = fields.next()?;
            let arguments = fields.next()?;
            (module_name == "module-remap-source"
                && arguments.contains("source_name=wreath_recording_"))
            .then(|| module_id.to_owned())
        })
        .collect()
}

fn default_desktop_monitor() -> Result<String, EngineError> {
    let output = Command::new("pactl")
        .args(["get-default-sink"])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        return Err(EngineError::Signal(format!(
            "cannot resolve the default desktop output: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let sink = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if sink.is_empty() {
        return Err(EngineError::Signal(
            "default desktop output has no PulseAudio name".into(),
        ));
    }
    Ok(format!("{sink}.monitor"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderCapabilities {
    pub vendor: String,
    pub video_codecs: Vec<String>,
}

impl GpuScreenRecorder {
    pub fn start(spec: &ReplaySpec) -> Result<Self, EngineError> {
        let executable =
            std::env::var_os("WREATH_RECORDER").unwrap_or_else(|| "gpu-screen-recorder".into());
        Self::start_with_executable(spec, executable)
    }

    fn start_with_executable(
        spec: &ReplaySpec,
        executable: impl AsRef<std::ffi::OsStr>,
    ) -> Result<Self, EngineError> {
        fs::create_dir_all(&spec.output_directory)?;
        let portal_session_token_file = (spec.monitor == "portal")
            .then(|| AppPaths::discover().cache_dir.join("portal-session-token"));
        if let Some(parent) = portal_session_token_file
            .as_deref()
            .and_then(std::path::Path::parent)
        {
            fs::create_dir_all(parent)?;
        }
        RecordingGainSource::unload_stale_sources();
        let microphone_gain_source = if spec.microphone_audio {
            let master = spec
                .microphone_device
                .as_deref()
                .unwrap_or("@DEFAULT_SOURCE@");
            Some(RecordingGainSource::create(
                master,
                spec.microphone_gain_percent,
                "Microphone",
            )?)
        } else {
            None
        };
        let mut recorder_spec = spec.clone();
        if let Some(source) = &microphone_gain_source {
            recorder_spec.microphone_device = Some(source.name.clone());
        }
        let desktop_gain_source = if spec.desktop_audio && spec.desktop_gain_percent != 100 {
            let master = match spec.desktop_device.as_deref() {
                Some(device) => device.to_owned(),
                None => default_desktop_monitor()?,
            };
            Some(RecordingGainSource::create(
                &master,
                spec.desktop_gain_percent,
                "Desktop",
            )?)
        } else {
            None
        };
        let mut child = Command::new(executable)
            .args(recorder_arguments(
                &recorder_spec,
                portal_session_token_file.as_deref(),
                desktop_gain_source
                    .as_ref()
                    .map(|source| source.name.as_str())
                    // A configured monitor source is recorded directly when it
                    // needs no level change of its own.
                    .or(spec.desktop_device.as_deref()),
            ))
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
            .name("wreath-save-reader".into())
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
            desktop_gain_source,
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
            self.desktop_gain_source.take();
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
        self.desktop_gain_source.take();
        Ok(())
    }
}

impl ReplayRecorder for GpuScreenRecorder {
    type Error = EngineError;

    fn is_running(&mut self) -> Result<bool, Self::Error> {
        GpuScreenRecorder::is_running(self)
    }

    fn save(&mut self) -> Result<PathBuf, Self::Error> {
        GpuScreenRecorder::save(self)
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        GpuScreenRecorder::stop(self)
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
            desktop_device: None,
            desktop_gain_percent: 100,
            microphone_audio: false,
            microphone_device: None,
            microphone_gain_percent: 100,
            output_directory: PathBuf::from("/tmp/wreath-test"),
        }
    }

    #[test]
    fn command_uses_direct_monitor_and_replay_mode() {
        let arguments = recorder_arguments(&spec(), None, None)
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
        let arguments = recorder_arguments(&replay, None, None)
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

    /// A configured output is recorded instead of whatever PipeWire currently
    /// calls the default sink.
    #[test]
    fn command_records_a_configured_output_directly() {
        let mut replay = spec();
        replay.desktop_device = Some("alsa_output.hdmi-stereo.monitor".into());
        let arguments = recorder_arguments(&replay, None, replay.desktop_device.as_deref())
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-a", "alsa_output.hdmi-stereo.monitor"])
        );
    }

    #[test]
    fn command_uses_an_isolated_desktop_source_when_provided() {
        let arguments = recorder_arguments(&spec(), None, Some("wreath_recording_Desktop_test"))
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .windows(2)
                .any(|pair| { pair == ["-a", "wreath_recording_Desktop_test"] })
        );
    }

    #[test]
    fn portal_capture_restores_the_users_session_choice() {
        let mut replay = spec();
        replay.monitor = "portal".into();
        let token_file = PathBuf::from("/tmp/wreath-portal-token");
        let arguments = recorder_arguments(&replay, Some(&token_file), None)
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
                .any(|pair| pair == ["-portal-session-token-filepath", "/tmp/wreath-portal-token"])
        );
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
    fn identifies_only_stale_wreath_recording_modules() {
        let modules = concat!(
            "10\tmodule-remap-source\tmaster=mic source_name=wreath_recording_mic_123 remix=yes\t\n",
            "13\tmodule-remap-source\tmaster=desktop source_name=wreath_recording_Desktop_123 remix=yes\t\n",
            "11\tmodule-remap-source\tmaster=mic source_name=other_app\t\n",
            "12\tmodule-null-sink\tsink_name=wreath_recording_mic_456\t\n",
        );
        assert_eq!(stale_wreath_module_ids(modules), ["10", "13"]);
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
            PathBuf::from("/tmp/wreath-test/clip.mp4")
        );
        recorder.stop().unwrap();
    }
}
