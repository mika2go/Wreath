use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

pub const MAX_FRAMES_PER_SECOND: u16 = 60;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub hotkey: HotkeyConfig,
    pub audio: AudioConfig,
    pub storage: StorageConfig,
    pub appearance: AppearanceConfig,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppearanceConfig {
    pub theme: Theme,
    pub hover: HoverStyle,
    pub hover_strength: HoverStrength,
    pub language: Language,
}

/// Interface language. `System` follows the Windows display language.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    German,
    English,
}

impl Language {
    pub const OPTIONS: [Self; 3] = [Self::System, Self::German, Self::English];
}

/// Interface palettes. The recorded video always carries the colour; the theme
/// only decides how the frame around it reads.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Cafe,
    Pink,
    Candy,
}

impl Theme {
    pub const OPTIONS: [Self; 5] = [Self::Dark, Self::Light, Self::Cafe, Self::Pink, Self::Candy];

    /// Themes that paint on a bright canvas, so the window frame follows them.
    pub const fn is_light(self) -> bool {
        matches!(self, Self::Light | Self::Candy)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HoverStyle {
    #[default]
    Surface,
    Outline,
    Both,
}

impl HoverStyle {
    pub const OPTIONS: [Self; 3] = [Self::Surface, Self::Outline, Self::Both];

    pub const fn fills(self) -> bool {
        matches!(self, Self::Surface | Self::Both)
    }

    pub const fn outlines(self) -> bool {
        matches!(self, Self::Outline | Self::Both)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HoverStrength {
    Off,
    Subtle,
    #[default]
    Normal,
    Strong,
}

impl HoverStrength {
    pub const OPTIONS: [Self; 4] = [Self::Off, Self::Subtle, Self::Normal, Self::Strong];

    pub const fn factor(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Subtle => 0.55,
            Self::Normal => 1.0,
            Self::Strong => 1.6,
        }
    }
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
    pub follow_game: bool,
    pub games: Vec<String>,
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
    pub desktop_device: Option<String>,
    pub desktop_gain_percent: u16,
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
            follow_game: true,
            games: Vec::new(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            modifiers: if cfg!(target_os = "windows") {
                vec!["CTRL".into(), "ALT".into()]
            } else {
                vec!["SUPER".into(), "SHIFT".into()]
            },
            key: "R".into(),
        }
    }
}

impl fmt::Display for HotkeyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_bound() {
            return formatter.write_str("Unbound");
        }
        for modifier in &self.modifiers {
            write!(formatter, "{modifier}+")?;
        }
        formatter.write_str(&self.key)
    }
}

impl HotkeyConfig {
    pub fn unbound() -> Self {
        Self {
            modifiers: Vec::new(),
            key: String::new(),
        }
    }

    pub fn is_bound(&self) -> bool {
        !self.key.is_empty()
    }

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
        if self.modifiers.is_empty() && self.key.is_empty() {
            return Ok(());
        }
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
                "hotkey must be an alphanumeric key name".into(),
            ));
        }
        Ok(())
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            desktop: true,
            desktop_device: None,
            desktop_gain_percent: 100,
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
        let mut config: Self = toml::from_str(&text).map_err(ConfigError::Parse)?;
        config.migrate();
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

    fn migrate(&mut self) {
        self.capture.frames_per_second = self.capture.frames_per_second.min(MAX_FRAMES_PER_SECOND);
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(5..=600).contains(&self.capture.duration_seconds) {
            return Err(ConfigError::Invalid(
                "clip duration must be between 5 and 600 seconds".into(),
            ));
        }
        if !(15..=MAX_FRAMES_PER_SECOND).contains(&self.capture.frames_per_second) {
            return Err(ConfigError::Invalid(format!(
                "frame rate must be between 15 and {MAX_FRAMES_PER_SECOND}"
            )));
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
        if self.audio.desktop_gain_percent > 200 {
            return Err(ConfigError::Invalid(
                "desktop recording level must be between 0 and 200 percent".into(),
            ));
        }
        self.hotkey.validate()?;
        if self.storage.max_megabytes < 128 {
            return Err(ConfigError::Invalid(
                "storage limit must be at least 128 MB".into(),
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
    fn appearance_defaults_to_the_dark_theme_with_a_normal_hover() {
        let appearance = Config::default().appearance;

        assert_eq!(appearance.theme, Theme::Dark);
        assert_eq!(appearance.hover, HoverStyle::Surface);
        assert_eq!(appearance.hover_strength, HoverStrength::Normal);
        assert_eq!(HoverStrength::Off.factor(), 0.0);
        assert!(HoverStyle::Surface.fills() && !HoverStyle::Surface.outlines());
        assert!(HoverStyle::Both.fills() && HoverStyle::Both.outlines());
    }

    #[test]
    fn a_configuration_without_an_appearance_section_keeps_the_defaults() {
        let config: Config = toml::from_str(
            r#"
[capture]
duration_seconds = 30
"#,
        )
        .expect("legacy configuration still parses");

        assert_eq!(config.appearance, AppearanceConfig::default());
    }

    #[test]
    fn the_appearance_section_round_trips_through_toml() {
        let mut config = Config::default();
        config.appearance.theme = Theme::Cafe;
        config.appearance.hover = HoverStyle::Outline;
        config.appearance.hover_strength = HoverStrength::Strong;

        let encoded = toml::to_string_pretty(&config).expect("configuration encodes");
        assert!(encoded.contains("theme = \"cafe\""));
        let decoded: Config = toml::from_str(&encoded).expect("configuration decodes");

        assert_eq!(decoded.appearance, config.appearance);
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
    fn invalid_desktop_recording_level_is_rejected() {
        let mut config = Config::default();
        config.audio.desktop_gain_percent = 201;
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
    fn older_audio_config_gets_a_unity_desktop_level() {
        let config: Config = toml::from_str("[audio]\ndesktop = true\n").unwrap();
        assert_eq!(config.audio.desktop_gain_percent, 100);
    }

    #[test]
    fn an_unnamed_output_follows_the_system_default() {
        let config: Config = toml::from_str("[audio]\ndesktop = true\n").unwrap();
        assert_eq!(config.audio.desktop_device, None);
    }

    #[test]
    fn a_retired_audio_setting_does_not_reject_the_configuration() {
        let config: Config =
            toml::from_str("[audio]\ndesktop = true\nexclude_discord = true\n").unwrap();

        assert!(config.audio.desktop);
    }

    #[test]
    fn an_old_high_frame_rate_configuration_is_brought_into_range_not_rejected() {
        let mut config = Config::default();
        config.capture.frames_per_second = 144;
        assert!(config.validate().is_err());

        config.migrate();

        assert_eq!(config.capture.frames_per_second, MAX_FRAMES_PER_SECOND);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn hotkey_parses_and_normalizes() {
        let hotkey = HotkeyConfig::parse("super + shift + r").unwrap();
        assert_eq!(hotkey.to_string(), "SUPER+SHIFT+R");
    }

    #[test]
    fn hotkey_can_be_fully_unbound() {
        let config = Config {
            hotkey: HotkeyConfig::unbound(),
            ..Config::default()
        };
        assert!(config.validate().is_ok());
        assert_eq!(config.hotkey.to_string(), "Unbound");
    }

    #[test]
    fn hotkey_rejects_code_injection() {
        assert!(HotkeyConfig::parse("SUPER+R\"); os.execute(\"bad").is_err());
    }
}
