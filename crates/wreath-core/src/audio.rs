use std::io;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Microphone {
    pub name: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOutput {
    pub name: String,
    pub label: String,
    pub is_default: bool,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct PulseSource {
    name: String,
    description: String,
    #[serde(default)]
    monitor_source: String,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct PulseSink {
    description: String,
    monitor_source: String,
    name: String,
}

#[cfg(target_os = "linux")]
pub fn desktop_outputs() -> io::Result<Vec<DesktopOutput>> {
    let output = Command::new("pactl")
        .args(["-f", "json", "list", "sinks"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("pactl could not list desktop outputs"));
    }
    let default = Command::new("pactl")
        .arg("get-default-sink")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    parse_sinks(&output.stdout, default.as_deref())
}

#[cfg(target_os = "windows")]
pub fn desktop_outputs() -> io::Result<Vec<DesktopOutput>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows desktop output discovery is not implemented in wreath-core",
    ))
}

#[cfg(target_os = "linux")]
pub fn microphones() -> io::Result<Vec<Microphone>> {
    let output = Command::new("pactl")
        .args(["-f", "json", "list", "sources"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("pactl could not list microphones"));
    }
    let default = Command::new("pactl")
        .arg("get-default-source")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    parse_sources(&output.stdout, default.as_deref())
}

#[cfg(target_os = "windows")]
pub fn microphones() -> io::Result<Vec<Microphone>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows microphone discovery is not implemented yet",
    ))
}

#[cfg(target_os = "linux")]
fn parse_sources(bytes: &[u8], default: Option<&str>) -> io::Result<Vec<Microphone>> {
    let sources: Vec<PulseSource> = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut microphones = sources
        .into_iter()
        .filter(|source| source.monitor_source.is_empty() && !source.name.ends_with(".monitor"))
        .map(|source| {
            let is_default = default == Some(source.name.as_str());
            Microphone {
                name: source.name,
                label: if is_default {
                    format!("{} · Default", source.description)
                } else {
                    source.description
                },
                is_default,
            }
        })
        .collect::<Vec<_>>();
    microphones.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(microphones)
}

#[cfg(target_os = "linux")]
fn parse_sinks(bytes: &[u8], default: Option<&str>) -> io::Result<Vec<DesktopOutput>> {
    let sinks: Vec<PulseSink> = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut outputs = sinks
        .into_iter()
        .filter(|sink| !sink.monitor_source.is_empty())
        .map(|sink| {
            let is_default = default == Some(sink.name.as_str());
            DesktopOutput {
                name: sink.monitor_source,
                label: if is_default {
                    format!("{} · Default", sink.description)
                } else {
                    sink.description
                },
                is_default,
            }
        })
        .collect::<Vec<_>>();
    outputs.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(outputs)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_physical_microphones_and_excludes_monitor_sources() {
        let sources = br#"[
          {
            "name": "alsa_output.card.monitor",
            "description": "Monitor of Speakers",
            "monitor_source": "alsa_output.card"
          },
          {
            "name": "alsa_input.usb-shure",
            "description": "Shure MV6 Mono",
            "monitor_source": ""
          },
          {
            "name": "alsa_input.onboard",
            "description": "Onboard Microphone"
          }
        ]"#;
        let microphones = parse_sources(sources, Some("alsa_input.usb-shure")).unwrap();
        assert_eq!(microphones.len(), 2);
        assert_eq!(microphones[0].name, "alsa_input.usb-shure");
        assert_eq!(microphones[0].label, "Shure MV6 Mono · Default");
        assert!(microphones[0].is_default);
        assert!(
            !microphones
                .iter()
                .any(|device| device.name.ends_with(".monitor"))
        );
    }

    #[test]
    fn maps_outputs_to_their_recordable_monitor_sources() {
        let sinks = br#"[
          {
            "name": "alsa_output.hdmi-stereo",
            "description": "Display Audio",
            "monitor_source": "alsa_output.hdmi-stereo.monitor"
          },
          {
            "name": "alsa_output.analog-stereo",
            "description": "Built-in Audio",
            "monitor_source": "alsa_output.analog-stereo.monitor"
          }
        ]"#;
        let outputs = parse_sinks(sinks, Some("alsa_output.analog-stereo")).unwrap();
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].name, "alsa_output.analog-stereo.monitor");
        assert_eq!(outputs[0].label, "Built-in Audio · Default");
        assert!(outputs[0].is_default);
    }
}
