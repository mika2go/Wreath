use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::config::HotkeyConfig;
use crate::display::Monitor;

const REPLAY_BIND_DESCRIPTION: &str = "Save Wreath replay";

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

pub fn session_active() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
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

pub fn replay_bind_present(hotkey: &HotkeyConfig) -> Result<bool, HyprlandError> {
    #[derive(Deserialize)]
    struct Bind {
        #[serde(default)]
        modmask: u32,
        #[serde(default)]
        key: String,
        #[serde(default)]
        description: String,
    }

    let output = Command::new("hyprctl")
        .args(["-j", "binds"])
        .output()
        .map_err(HyprlandError::Io)?;
    if !output.status.success() {
        return Err(HyprlandError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let binds: Vec<Bind> = serde_json::from_slice(&output.stdout).map_err(HyprlandError::Json)?;
    let expected_mask = modifier_mask(&hotkey.modifiers);
    Ok(binds.iter().any(|bind| {
        bind.description == REPLAY_BIND_DESCRIPTION
            && bind.modmask == expected_mask
            && bind.key.eq_ignore_ascii_case(&hotkey.key)
    }))
}

fn modifier_mask(modifiers: &[String]) -> u32 {
    modifiers.iter().fold(0, |mask, modifier| {
        mask | match modifier.as_str() {
            "SHIFT" => 1,
            "CTRL" => 4,
            "ALT" => 8,
            "SUPER" => 64,
            _ => 0,
        }
    })
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
         {{ description = \"Save Wreath replay\" }})",
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

    #[test]
    fn escaping_keeps_paths_inside_lua_and_shell_strings() {
        assert_eq!(lua_escape("a\"b"), "a\\\"b");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn replay_bind_replaces_old_and_current_hotkeys() {
        let previous = HotkeyConfig::parse("SUPER+SHIFT+R").unwrap();
        let current = HotkeyConfig::parse("SUPER+ALT+C").unwrap();
        let code = replay_bind_code(Some(&previous), &current, "'/usr/bin/wreathctl' save");
        assert!(code.contains("hl.unbind(\"SUPER + SHIFT + R\")"));
        assert!(code.contains("hl.unbind(\"SUPER + ALT + C\")"));
        assert!(code.contains("hl.bind(\"SUPER + ALT + C\""));
    }

    #[test]
    fn modifier_masks_match_hyprland_bind_masks() {
        assert_eq!(modifier_mask(&["CTRL".into()]), 4);
        assert_eq!(modifier_mask(&["SUPER".into(), "SHIFT".into()]), 65);
        assert_eq!(
            modifier_mask(&["SUPER".into(), "CTRL".into(), "ALT".into()]),
            76
        );
    }
}
