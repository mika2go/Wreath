use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CaptureConfig {
    pub monitor: Option<String>,
    pub duration_seconds: u16,
    pub frames_per_second: u16,
    pub codec: Codec,
    pub quality: u8,
    pub cursor: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Codec {
    Auto,
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HotkeyConfig {
    pub modifiers: Vec<String>,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioConfig {
    pub desktop: bool,
    pub microphone: bool,
    pub microphone_device: Option<String>,
    pub microphone_gain_percent: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    pub directory: PathBuf,
    pub max_megabytes: u32,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
    Encode(toml::ser::Error),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Parse(error) => write!(formatter, "invalid configuration: {error}"),
            Self::Encode(error) => write!(formatter, "cannot encode configuration: {error}"),
            Self::Invalid(message) => write!(formatter, "invalid configuration: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            monitor: None,
            duration_seconds: 30,
            frames_per_second: 60,
            codec: Codec::Auto,
            quality: 75,
            cursor: true,
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            modifiers: vec!["SUPER".into(), "SHIFT".into()],
            key: "R".into(),
        }
    }
}

impl fmt::Display for HotkeyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(formatter, "{modifier}+")?;
        }
        formatter.write_str(&self.key)
    }
}

impl HotkeyConfig {
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let parts = value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let (key, modifiers) = parts
            .split_last()
            .ok_or_else(|| ConfigError::Invalid("hotkey cannot be empty".into()))?;
        let hotkey = Self {
            modifiers: modifiers
                .iter()
                .map(|modifier| modifier.to_ascii_uppercase())
                .collect(),
            key: key.to_ascii_uppercase(),
        };
        hotkey.validate()?;
        Ok(hotkey)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        const MODIFIERS: &[&str] = &["SUPER", "SHIFT", "CTRL", "ALT"];
        if self
            .modifiers
            .iter()
            .any(|modifier| !MODIFIERS.contains(&modifier.as_str()))
        {
            return Err(ConfigError::Invalid(
                "hotkey modifiers must be SUPER, SHIFT, CTRL, or ALT".into(),
            ));
        }
        if self.modifiers.len() > MODIFIERS.len() {
            return Err(ConfigError::Invalid("too many hotkey modifiers".into()));
        }
        if self.key.is_empty()
            || !self
                .key
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(ConfigError::Invalid(
                "hotkey must be an alphanumeric XKB key name".into(),
            ));
        }
        Ok(())
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            desktop: true,
            microphone: false,
            microphone_device: None,
            microphone_gain_percent: 100,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        let directory = default_video_directory();
        Self {
            directory,
            max_megabytes: 10_240,
        }
    }
}

fn default_video_directory() -> PathBuf {
    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME");

    home.map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Videos")
        .join("Wreath")
}

impl Config {
    pub fn load(paths: &AppPaths) -> Result<Self, ConfigError> {
        let source = std::iter::once(&paths.config_file)
            .chain(paths.legacy_config_files.iter())
            .find(|path| path.exists());
        let Some(source) = source else {
            return Ok(Self::default());
        };
        let text = fs::read_to_string(source)?;
        let config: Self = toml::from_str(&text).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, paths: &AppPaths) -> Result<(), ConfigError> {
        self.validate()?;
        fs::create_dir_all(&paths.config_dir)?;
        let encoded = toml::to_string_pretty(self).map_err(ConfigError::Encode)?;
        atomic_write(&paths.config_file, encoded.as_bytes())?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(5..=600).contains(&self.capture.duration_seconds) {
            return Err(ConfigError::Invalid(
                "clip duration must be between 5 and 600 seconds".into(),
            ));
        }
        if !(15..=240).contains(&self.capture.frames_per_second) {
            return Err(ConfigError::Invalid(
                "frame rate must be between 15 and 240".into(),
            ));
        }
        if self.capture.quality > 100 {
            return Err(ConfigError::Invalid(
                "quality must be between 0 and 100".into(),
            ));
        }
        if self.audio.microphone_gain_percent > 200 {
            return Err(ConfigError::Invalid(
                "microphone recording level must be between 0 and 200 percent".into(),
            ));
        }
        self.hotkey.validate()?;
        if self.storage.max_megabytes < 128 {
            return Err(ConfigError::Invalid(
                "storage limit must be at least 128 MiB".into(),
            ));
        }
        if !self.storage.directory.is_absolute() {
            return Err(ConfigError::Invalid(
                "storage directory must be an absolute local path".into(),
            ));
        }
        Ok(())
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn invalid_duration_is_rejected() {
        let mut config = Config::default();
        config.capture.duration_seconds = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn invalid_microphone_recording_level_is_rejected() {
        let mut config = Config::default();
        config.audio.microphone_gain_percent = 201;
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_round_trips() {
        let config = Config::default();
        let encoded = toml::to_string(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, config);
    }

    #[test]
    fn hotkey_parses_and_normalizes() {
        let hotkey = HotkeyConfig::parse("super + shift + r").unwrap();
        assert_eq!(hotkey.to_string(), "SUPER+SHIFT+R");
    }

    #[test]
    fn hotkey_rejects_code_injection() {
        assert!(HotkeyConfig::parse("SUPER+R\"); os.execute(\"bad").is_err());
    }
}
