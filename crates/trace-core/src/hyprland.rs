use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::HotkeyConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
    pub focused: bool,
    pub disabled: bool,
}

#[derive(Debug)]
pub enum HyprlandError {
    Io(io::Error),
    Command(String),
    Json(serde_json::Error),
}

impl fmt::Display for HyprlandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot run hyprctl: {error}"),
            Self::Command(message) => write!(formatter, "hyprctl failed: {message}"),
            Self::Json(error) => write!(formatter, "invalid hyprctl response: {error}"),
        }
    }
}

impl std::error::Error for HyprlandError {}

pub fn monitors() -> Result<Vec<Monitor>, HyprlandError> {
    let output = Command::new("hyprctl")
        .args(["-j", "monitors", "all"])
        .output()
        .map_err(HyprlandError::Io)?;
    if !output.status.success() {
        return Err(HyprlandError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let mut monitors: Vec<Monitor> =
        serde_json::from_slice(&output.stdout).map_err(HyprlandError::Json)?;
    monitors.retain(|monitor| !monitor.disabled);
    monitors.sort_by_key(|monitor| monitor.id);
    Ok(monitors)
}

pub fn resolve_monitor<'a>(
    monitors: &'a [Monitor],
    configured: Option<&str>,
) -> Option<&'a Monitor> {
    configured
        .and_then(|needle| {
            monitors
                .iter()
                .find(|monitor| monitor.description == needle || monitor.name == needle)
        })
        .or_else(|| monitors.iter().find(|monitor| monitor.focused))
        .or_else(|| monitors.first())
}

pub fn install_replay_bind(
    hotkey: &HotkeyConfig,
    control_executable: &Path,
) -> Result<(), HyprlandError> {
    replace_replay_bind(None, hotkey, control_executable)
}

pub fn replace_replay_bind(
    previous_hotkey: Option<&HotkeyConfig>,
    hotkey: &HotkeyConfig,
    control_executable: &Path,
) -> Result<(), HyprlandError> {
    let executable = shell_quote(&control_executable.to_string_lossy());
    let command = format!("{executable} save");
    let code = replay_bind_code(previous_hotkey, hotkey, &command);
    let output = Command::new("hyprctl")
        .args(["eval", &code])
        .output()
        .map_err(HyprlandError::Io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(HyprlandError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

fn replay_bind_code(
    previous_hotkey: Option<&HotkeyConfig>,
    hotkey: &HotkeyConfig,
    command: &str,
) -> String {
    let remove_previous = previous_hotkey
        .filter(|previous| *previous != hotkey)
        .map(|previous| {
            format!(
                "hl.unbind(\"{}\"); ",
                lua_escape(&previous.hyprland_expression())
            )
        })
        .unwrap_or_default();
    format!(
        "{remove_previous}hl.unbind(\"{}\"); \
         hl.bind(\"{}\", hl.dsp.exec_cmd(\"{}\"), \
         {{ description = \"Save Trace replay\" }})",
        lua_escape(&hotkey.hyprland_expression()),
        lua_escape(&hotkey.hyprland_expression()),
        lua_escape(command)
    )
}

fn lua_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, description: &str, focused: bool) -> Monitor {
        Monitor {
            id: 0,
            name: name.into(),
            description: description.into(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            focused,
            disabled: false,
        }
    }

    #[test]
    fn resolves_stable_description_before_connector_name() {
        let monitors = vec![
            monitor("DP-1", "Display A", false),
            monitor("DP-2", "Display B", true),
        ];
        assert_eq!(
            resolve_monitor(&monitors, Some("Display A")).unwrap().name,
            "DP-1"
        );
    }

    #[test]
    fn falls_back_to_focused_monitor() {
        let monitors = vec![
            monitor("DP-1", "Display A", false),
            monitor("DP-2", "Display B", true),
        ];
        assert_eq!(resolve_monitor(&monitors, None).unwrap().name, "DP-2");
    }

    #[test]
    fn escaping_keeps_paths_inside_lua_and_shell_strings() {
        assert_eq!(lua_escape("a\"b"), "a\\\"b");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn replay_bind_replaces_old_and_current_hotkeys() {
        let previous = HotkeyConfig::parse("SUPER+SHIFT+R").unwrap();
        let current = HotkeyConfig::parse("SUPER+ALT+C").unwrap();
        let code = replay_bind_code(Some(&previous), &current, "'/usr/bin/tracectl' save");
        assert!(code.contains("hl.unbind(\"SUPER + SHIFT + R\")"));
        assert!(code.contains("hl.unbind(\"SUPER + ALT + C\")"));
        assert!(code.contains("hl.bind(\"SUPER + ALT + C\""));
    }
}
