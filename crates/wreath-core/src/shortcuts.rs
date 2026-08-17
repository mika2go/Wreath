use std::fmt;
use std::path::Path;

use crate::config::HotkeyConfig;
use crate::hyprland::{self, HyprlandError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutBackend {
    Hyprland,
    Plasma,
    Manual(String),
}

impl fmt::Display for ShortcutBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyprland => formatter.write_str("Hyprland native bind"),
            Self::Plasma => formatter.write_str("KDE Plasma System Settings"),
            Self::Manual(desktop) => write!(formatter, "{desktop} manual shortcut"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutInstall {
    Installed,
    Manual {
        backend: ShortcutBackend,
        command: String,
    },
}

#[derive(Debug)]
pub enum ShortcutError {
    Hyprland(HyprlandError),
}

impl fmt::Display for ShortcutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hyprland(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ShortcutError {}

pub fn backend() -> ShortcutBackend {
    if hyprland::session_active() {
        return ShortcutBackend::Hyprland;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_else(|_| "this desktop".into());
    let normalized = desktop.to_ascii_lowercase();
    if normalized.contains("kde") || normalized.contains("plasma") {
        ShortcutBackend::Plasma
    } else {
        ShortcutBackend::Manual(desktop)
    }
}

pub fn install(
    hotkey: &HotkeyConfig,
    control_executable: &Path,
) -> Result<ShortcutInstall, ShortcutError> {
    replace(None, hotkey, control_executable)
}

pub fn ensure(hotkey: &HotkeyConfig, control_executable: &Path) -> Result<bool, ShortcutError> {
    if !hotkey.is_bound() {
        return Ok(false);
    }
    if !matches!(backend(), ShortcutBackend::Hyprland) {
        return Ok(false);
    }
    if hyprland::replay_bind_present(hotkey).map_err(ShortcutError::Hyprland)? {
        return Ok(false);
    }
    hyprland::replace_replay_bind(None, hotkey, control_executable)
        .map_err(ShortcutError::Hyprland)?;
    Ok(true)
}

pub fn replace(
    previous_hotkey: Option<&HotkeyConfig>,
    hotkey: &HotkeyConfig,
    control_executable: &Path,
) -> Result<ShortcutInstall, ShortcutError> {
    match backend() {
        ShortcutBackend::Hyprland => {
            hyprland::replace_replay_bind(previous_hotkey, hotkey, control_executable)
                .map_err(ShortcutError::Hyprland)?;
            Ok(ShortcutInstall::Installed)
        }
        backend => Ok(ShortcutInstall::Manual {
            backend,
            command: format!("{} save", control_executable.display()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_installation_exposes_the_exact_save_command() {
        let installation = ShortcutInstall::Manual {
            backend: ShortcutBackend::Plasma,
            command: "/usr/bin/wreathctl save".into(),
        };

        assert_eq!(
            installation,
            ShortcutInstall::Manual {
                backend: ShortcutBackend::Plasma,
                command: "/usr/bin/wreathctl save".into(),
            }
        );
    }
}
