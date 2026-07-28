use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::config::{Codec, Config};
use crate::hyprland::Monitor;

const SAVE_TIMEOUT: Duration = Duration::from_secs(15);

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
    pub output_directory: PathBuf,
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
                "gpu-screen-recorder is not installed; on Arch run `sudo pacman -S gpu-screen-recorder`"
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
            output_directory: config.storage.directory.clone(),
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
        if self.desktop_audio {
            arguments.extend([
                "-a".into(),
                "default_output".into(),
                "-ac".into(),
                "aac".into(),
            ]);
        }
        if self.microphone_audio {
            arguments.extend([
                "-a".into(),
                self.microphone_device
                    .as_deref()
                    .unwrap_or("default_input")
                    .into(),
            ]);
        }
        arguments
    }

    pub fn target_bitrate_kbps(&self) -> u32 {
        let pixels_per_second =
            u64::from(self.width) * u64::from(self.height) * u64::from(self.frames_per_second);
        let codec_factor = match self.codec {
            Codec::Auto | Codec::H264 => 100_u64,
            Codec::Hevc => 72,
            Codec::Av1 => 58,
        };
        let quality_factor = 35_u64 + u64::from(self.quality);
        let bitrate = pixels_per_second
            .saturating_mul(codec_factor)
            .saturating_mul(quality_factor)
            / 1_000_000;
        u32::try_from(bitrate.clamp(2_500, 120_000)).unwrap_or(120_000)
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
}

impl GpuScreenRecorder {
    pub fn start(spec: &ReplaySpec) -> Result<Self, EngineError> {
        fs::create_dir_all(&spec.output_directory)?;
        let executable =
            std::env::var_os("RIFTCLIP_RECORDER").unwrap_or_else(|| "gpu-screen-recorder".into());
        let mut child = Command::new(executable)
            .args(spec.arguments())
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
            .name("riftclip-save-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        let _ = saved_sender.send(PathBuf::from(trimmed));
                    }
                }
            })?;
        Ok(Self { child, saved_paths })
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
            return Ok(());
        }
        send_signal(self.process_id(), "INT")?;
        self.child.wait()?;
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

pub fn recorder_available() -> bool {
    Command::new("gpu-screen-recorder")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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
            output_directory: PathBuf::from("/tmp/riftclip-test"),
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
    fn buffer_estimate_is_bounded_and_nonzero() {
        let replay = spec();
        assert!(replay.target_bitrate_kbps() >= 2_500);
        assert!(replay.estimated_buffer_megabytes() > 0);
        assert!(replay.estimated_buffer_megabytes() < 1_000);
    }
}
