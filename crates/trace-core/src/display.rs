use std::fmt;
use std::io;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::hyprland;

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

impl Monitor {
    pub fn uses_portal(&self) -> bool {
        self.name == "portal"
    }
}

#[derive(Debug)]
pub enum DisplayError {
    Io(io::Error),
    Recorder(String),
    NoCaptureTargets,
}

impl fmt::Display for DisplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) if error.kind() == io::ErrorKind::NotFound => write!(
                formatter,
                "gpu-screen-recorder is not installed; on Arch or CachyOS run \
                 `sudo pacman -S gpu-screen-recorder`"
            ),
            Self::Io(error) => write!(formatter, "cannot inspect displays: {error}"),
            Self::Recorder(message) => {
                write!(
                    formatter,
                    "gpu-screen-recorder display query failed: {message}"
                )
            }
            Self::NoCaptureTargets => write!(
                formatter,
                "no recordable display found; verify your GPU driver and desktop portal"
            ),
        }
    }
}

impl std::error::Error for DisplayError {}

pub fn monitors() -> Result<Vec<Monitor>, DisplayError> {
    if hyprland::session_active()
        && let Ok(monitors) = hyprland::monitors()
        && !monitors.is_empty()
    {
        return Ok(monitors);
    }
    recorder_monitors()
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
        .or_else(|| monitors.iter().find(|monitor| !monitor.uses_portal()))
        .or_else(|| monitors.first())
}

fn recorder_monitors() -> Result<Vec<Monitor>, DisplayError> {
    let output = Command::new("gpu-screen-recorder")
        .arg("--info")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(DisplayError::Io)?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(DisplayError::Recorder(if message.is_empty() {
            format!("process exited with {}", output.status)
        } else {
            message
        }));
    }
    let monitors = parse_capture_options(&String::from_utf8_lossy(&output.stdout));
    if monitors.is_empty() {
        Err(DisplayError::NoCaptureTargets)
    } else {
        Ok(monitors)
    }
}

fn parse_capture_options(output: &str) -> Vec<Monitor> {
    let mut section = "";
    let mut direct = Vec::new();
    let mut portal_available = false;
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = line.strip_prefix("section=") {
            section = value;
            continue;
        }
        if section != "capture_options" {
            continue;
        }
        if line == "portal" {
            portal_available = true;
            continue;
        }
        let Some((name, dimensions)) = line.split_once('|') else {
            continue;
        };
        let Some((width, height)) = parse_dimensions(dimensions) else {
            continue;
        };
        direct.push(Monitor {
            id: i32::try_from(direct.len()).unwrap_or(i32::MAX),
            name: name.to_owned(),
            description: name.to_owned(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width,
            height,
            refresh_rate: 60.0,
            focused: direct.is_empty(),
            disabled: false,
        });
    }
    if portal_available {
        direct.push(Monitor {
            id: i32::try_from(direct.len()).unwrap_or(i32::MAX),
            name: "portal".into(),
            description: "Desktop portal".into(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            width: 1920,
            height: 1080,
            refresh_rate: 60.0,
            focused: direct.is_empty(),
            disabled: false,
        });
    }
    direct
}

fn parse_dimensions(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
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
    fn parses_direct_and_portal_capture_targets() {
        let targets = parse_capture_options(
            "section=system_info\n\
             display_server|wayland\n\
             section=capture_options\n\
             DP-1|2560x1440\n\
             HDMI-A-1|1920x1080\n\
             region\n\
             portal\n",
        );

        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].name, "DP-1");
        assert_eq!((targets[0].width, targets[0].height), (2560, 1440));
        assert!(targets[0].focused);
        assert!(targets[2].uses_portal());
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
}
