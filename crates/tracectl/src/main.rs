use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Stdio;
use std::process::{Command, ExitCode};

use trace_core::hyprland;
use trace_core::ipc::{Request, Response};
use trace_core::paths::AppPaths;
use trace_core::{config::Codec, config::Config, config::HotkeyConfig};
use trace_core::{engine, hyprland::resolve_monitor};

const SOUND_SAMPLE_RATE: usize = 48_000;
const SOUND_DURATION_SECONDS: f32 = 0.48;
const SOUND_PLAYBACK_VOLUME: &str = "0.14";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("tracectl: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let command = arguments.first().map(String::as_str).unwrap_or("status");
    if command == "monitors" {
        for monitor in hyprland::monitors().map_err(|error| error.to_string())? {
            let focused = if monitor.focused { " [focused]" } else { "" };
            println!(
                "{} — {} ({}x{} @ {:.0} Hz){}",
                monitor.name,
                monitor.description,
                monitor.width,
                monitor.height,
                monitor.refresh_rate,
                focused
            );
        }
        return Ok(());
    }
    if command == "bind" {
        let paths = AppPaths::discover();
        let config = Config::load(&paths).map_err(|error| error.to_string())?;
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        hyprland::install_replay_bind(&config.hotkey, &executable)
            .map_err(|error| error.to_string())?;
        println!("bound {} in Hyprland", config.hotkey);
        return Ok(());
    }
    if command == "config" {
        return configure(&arguments[1..]);
    }
    if command == "doctor" {
        return doctor();
    }
    if command == "sound" {
        play_clip_saved_sound();
        return Ok(());
    }

    let request = match command {
        "status" => Request::Status,
        "save" => Request::Save,
        "pause" => Request::Pause,
        "resume" => Request::Resume,
        "reload" => Request::Reload,
        "shutdown" => Request::Shutdown,
        "help" | "--help" | "-h" => {
            print_help();
            return Ok(());
        }
        other => return Err(format!("unknown command `{other}`; try `tracectl help`")),
    };

    let response = send(&AppPaths::discover(), &request)?;
    match response {
        Response::Status {
            state,
            monitor,
            buffered_seconds,
            error,
        } => {
            println!(
                "{state:?}: monitor={}, buffer={}s",
                monitor.as_deref().unwrap_or("none"),
                buffered_seconds
            );
            if let Some(error) = error {
                println!("error: {error}");
            }
        }
        Response::Saved { path } => {
            println!("{}", path.display());
            show_clip_saved_feedback(&path);
        }
        Response::Ok => {}
        Response::Error { message } => return Err(message),
    }
    Ok(())
}

fn show_clip_saved_feedback(path: &Path) {
    let detail = clip_saved_detail(path);

    spawn_quiet(
        "notify-send",
        &[
            "--app-name=Trace",
            "--icon=io.github.mika2go.Trace-symbolic",
            "--urgency=normal",
            "--expire-time=3000",
            "Clip taken",
            &detail,
        ],
    );
    play_clip_saved_sound();
}

fn clip_saved_detail(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("Saved {name}"))
        .unwrap_or_else(|| "Replay saved successfully".to_owned())
}

fn spawn_quiet(program: &str, arguments: &[&str]) {
    let _ = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn play_clip_saved_sound() {
    let Ok(mut player) = Command::new("pw-play")
        .args([
            "--raw",
            "--format=s16",
            "--rate=48000",
            "--channels=1",
            "--volume",
            SOUND_PLAYBACK_VOLUME,
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut input) = player.stdin.take() {
        let _ = input.write_all(&render_clip_saved_sound());
    }
}

fn render_clip_saved_sound() -> Vec<u8> {
    let sample_count = (SOUND_SAMPLE_RATE as f32 * SOUND_DURATION_SECONDS) as usize;
    let mut audio = Vec::with_capacity(sample_count * size_of::<i16>());
    for index in 0..sample_count {
        let time = index as f32 / SOUND_SAMPLE_RATE as f32;
        let first = chime_tone(time, 0.0, 739.99, 0.30, 0.066);
        let second = chime_tone(time, 0.12, 1108.73, 0.36, 0.058);
        let sample = ((first + second).clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        audio.extend_from_slice(&sample.to_le_bytes());
    }
    audio
}

fn chime_tone(time: f32, start: f32, frequency: f32, duration: f32, level: f32) -> f32 {
    let local_time = time - start;
    if !(0.0..duration).contains(&local_time) {
        return 0.0;
    }
    let attack = (local_time / 0.012).min(1.0);
    let release = ((duration - local_time) / 0.09).min(1.0);
    let decay = (-5.2 * local_time).exp();
    let phase = std::f32::consts::TAU * frequency * local_time;
    let timbre = phase.sin() + 0.16 * (phase * 2.0 + 0.35).sin();
    level * attack * release * decay * timbre
}

fn doctor() -> Result<(), String> {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).map_err(|error| error.to_string())?;
    println!("config  ok · {}", paths.config_file.display());
    let monitors = hyprland::monitors().map_err(|error| error.to_string())?;
    println!("hyprland ok · {} active monitor(s)", monitors.len());
    let monitor = resolve_monitor(&monitors, config.capture.monitor.as_deref())
        .ok_or_else(|| "no active monitor found".to_owned())?;
    println!(
        "capture  ok · {} · {}x{} @ {:.0} Hz",
        monitor.name, monitor.width, monitor.height, monitor.refresh_rate
    );
    println!(
        "buffer   ok · about {} MiB for {} seconds",
        engine::ReplaySpec::from_config(&config, monitor).estimated_buffer_megabytes(),
        config.capture.duration_seconds
    );
    let capabilities = engine::recorder_capabilities().map_err(|error| error.to_string())?;
    let codecs = ["h264", "hevc", "av1"]
        .into_iter()
        .filter(|codec| capabilities.video_codecs.iter().any(|item| item == codec))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "gpu      ok · {} · hardware codecs: {}",
        capabilities.vendor,
        if codecs.is_empty() {
            "automatic"
        } else {
            &codecs
        }
    );
    println!("engine   ok · gpu-screen-recorder · AMD/NVIDIA auto detection");
    println!("network  blocked by packaged systemd user service");
    Ok(())
}

fn configure(arguments: &[String]) -> Result<(), String> {
    let paths = AppPaths::discover();
    let mut config = Config::load(&paths).map_err(|error| error.to_string())?;
    let previous_hotkey = config.hotkey.clone();
    let Some(setting) = arguments.first().map(String::as_str) else {
        let encoded = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
        print!("{encoded}");
        return Ok(());
    };
    let value = arguments
        .get(1)
        .ok_or_else(|| format!("missing value for `{setting}`"))?;
    match setting {
        "monitor" => {
            let monitors = hyprland::monitors().map_err(|error| error.to_string())?;
            let monitor = monitors
                .iter()
                .find(|monitor| monitor.name == *value || monitor.description == *value)
                .ok_or_else(|| format!("unknown monitor `{value}`; run `tracectl monitors`"))?;
            config.capture.monitor = Some(monitor.description.clone());
        }
        "hotkey" => {
            config.hotkey = HotkeyConfig::parse(value).map_err(|error| error.to_string())?;
        }
        "duration" => {
            config.capture.duration_seconds = parse_number(value, "duration")?;
        }
        "fps" => {
            config.capture.frames_per_second = parse_number(value, "fps")?;
        }
        "quality" => {
            config.capture.quality = parse_number(value, "quality")?;
        }
        "codec" => {
            config.capture.codec = match value.as_str() {
                "auto" => Codec::Auto,
                "h264" => Codec::H264,
                "hevc" => Codec::Hevc,
                "av1" => Codec::Av1,
                _ => return Err("codec must be auto, h264, hevc, or av1".into()),
            };
        }
        "output" => config.storage.directory = value.into(),
        _ => {
            return Err(format!(
                "unknown setting `{setting}`; use monitor, hotkey, duration, fps, quality, codec, or output"
            ));
        }
    }
    config.save(&paths).map_err(|error| error.to_string())?;
    if setting == "hotkey" {
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        hyprland::replace_replay_bind(Some(&previous_hotkey), &config.hotkey, &executable)
            .map_err(|error| error.to_string())?;
    }
    let _ = send(&paths, &Request::Reload);
    println!("saved {setting} in {}", paths.config_file.display());
    Ok(())
}

fn parse_number<T>(value: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("{name} must be a number"))
}

fn send(paths: &AppPaths, request: &Request) -> Result<Response, String> {
    let mut stream = match UnixStream::connect(&paths.socket_file) {
        Ok(stream) => stream,
        Err(first_error) if matches!(request, Request::Save) => {
            start_daemon().map_err(|start_error| {
                format!(
                    "cannot connect to {}: {first_error}; could not start recorder: {start_error}",
                    paths.socket_file.display()
                )
            })?;
            UnixStream::connect(&paths.socket_file).map_err(|error| {
                format!(
                    "recorder started but cannot connect to {}: {error}",
                    paths.socket_file.display()
                )
            })?
        }
        Err(error) => {
            return Err(format!(
                "cannot connect to {}: {error}",
                paths.socket_file.display()
            ));
        }
    };
    serde_json::to_writer(&mut stream, request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&line).map_err(|error| format!("invalid daemon response: {error}"))
}

fn start_daemon() -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(["--user", "start", "traced.service"])
        .output()
        .map_err(|error| format!("cannot run systemctl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if message.is_empty() {
            format!("systemctl exited with {}", output.status)
        } else {
            message
        })
    }
}

fn print_help() {
    println!(
        "tracectl <command>\n\n\
         commands:\n  monitors  list Hyprland monitors\n  status    show daemon state\n  \
         save      save the replay buffer\n  pause     pause capture\n  resume    resume capture\n  \
         reload    reload local configuration\n  bind      register configured Hyprland hotkey\n  \
         sound     preview the clip confirmation sound\n  \
         config    show or change local settings\n  doctor    verify the local runtime\n  \
         shutdown  stop the daemon\n\n\
         examples:\n  tracectl config monitor DP-1\n  tracectl config hotkey SUPER+SHIFT+R\n  \
         tracectl config duration 30\n  tracectl config fps 60\n  \
         tracectl config codec av1"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_clip_feedback_uses_the_output_filename() {
        let path = Path::new("/tmp/Trace Replay 2026-07-29.mp4");

        assert_eq!(clip_saved_detail(path), "Saved Trace Replay 2026-07-29.mp4");
    }

    #[test]
    fn generated_clip_sound_is_short_and_safely_below_full_scale() {
        let audio = render_clip_saved_sound();
        let samples = audio
            .chunks_exact(size_of::<i16>())
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        let peak = samples.iter().map(|sample| sample.unsigned_abs()).max();

        assert_eq!(
            samples.len(),
            (SOUND_SAMPLE_RATE as f32 * SOUND_DURATION_SECONDS) as usize
        );
        assert!(peak.is_some_and(|peak| peak > 1_000 && peak < 8_000));
        assert!(samples.first().is_some_and(|sample| sample.abs() < 8));
        assert!(samples.last().is_some_and(|sample| sample.abs() < 40));
    }
}
