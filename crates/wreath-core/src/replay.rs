use std::error::Error;
use std::path::PathBuf;

use crate::config::{Codec, Config};
use crate::display::Monitor;

/// Platform-neutral description of the replay stream Wreath should keep resident.
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
        }
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

    pub fn estimated_buffer_bytes(&self) -> u64 {
        u64::from(self.target_bitrate_kbps())
            .saturating_mul(1_000)
            .saturating_mul(u64::from(self.duration_seconds))
            / 8
    }

    pub fn estimated_buffer_megabytes(&self) -> u64 {
        self.estimated_buffer_bytes() / 1_048_576
    }
}

/// A running platform recorder. Implementations may be an external Linux process
/// or an in-process Windows capture pipeline.
pub trait ReplayRecorder {
    type Error: Error;

    fn is_running(&mut self) -> Result<bool, Self::Error>;
    fn save(&mut self) -> Result<PathBuf, Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
}

/// Creates the platform recorder while keeping daemon lifecycle logic independent
/// from the capture API used by the operating system.
pub trait ReplayBackend {
    type Error: Error;
    type Recorder: ReplayRecorder<Error = Self::Error>;

    fn start(&self, spec: &ReplaySpec) -> Result<Self::Recorder, Self::Error>;
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
            output_directory: PathBuf::from("/tmp/wreath-test"),
        }
    }

    #[test]
    fn buffer_estimate_is_bounded_and_nonzero() {
        let replay = spec();
        assert!(replay.target_bitrate_kbps() >= 2_500);
        assert!(replay.estimated_buffer_bytes() > 0);
        assert!(replay.estimated_buffer_megabytes() < 100);
    }
}
