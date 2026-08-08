#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::io::{BufReader, Write};
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::process::ExitCode;
#[cfg(target_os = "linux")]
use std::process::Stdio;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use wreath_core::display;
#[cfg(target_os = "linux")]
use wreath_core::engine;
#[cfg(target_os = "linux")]
use wreath_core::ipc::{self, Request, Response};
#[cfg(target_os = "linux")]
use wreath_core::paths::AppPaths;
#[cfg(target_os = "linux")]
use wreath_core::replay::ReplaySpec;
#[cfg(target_os = "linux")]
use wreath_core::shortcuts::{self, ShortcutInstall};
#[cfg(target_os = "linux")]
use wreath_core::{config::Codec, config::Config, config::HotkeyConfig};

mod cut;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
const SOUND_SAMPLE_RATE: usize = 48_000;
#[cfg(target_os = "linux")]
const SOUND_DURATION_SECONDS: f32 = 0.48;
#[cfg(target_os = "linux")]
const SOUND_PLAYBACK_VOLUME: &str = "9175";
#[cfg(target_os = "linux")]
const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "linux")]
const DAEMON_CONNECT_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("wreathctl: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    match windows::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("wreathctl: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let command = arguments.first().map(String::as_str).unwrap_or("status");
    if command == "monitors" {
        for monitor in display::monitors().map_err(|error| error.to_string())? {
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
        match shortcuts::install(&config.hotkey, &executable).map_err(|error| error.to_string())? {
            ShortcutInstall::Installed => println!("bound {} in Hyprland", config.hotkey),
            ShortcutInstall::Manual { backend, command } => {
                println!(
                    "shortcut requires {backend}; assign {} to `{command}`",
                    config.hotkey
                );
            }
        }
        return Ok(());
    }
    if command == "config" {
        return configure(&arguments[1..]);
    }
    if command == "cut" {
        return cut::run(&arguments[1..]);
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
        other => return Err(format!("unknown command `{other}`; try `wreathctl help`")),
    };

    let response = send(&AppPaths::discover(), &request)?;
    match response {
        Response::Status {
            state,
            monitor,
            codec,
            adapter,
            replay_bytes,
            buffered_seconds,
            error,
        } => {
            println!(
                "{state:?}: monitor={}, buffer={}s",
                monitor.as_deref().unwrap_or("none"),
                buffered_seconds
            );
            if let Some(codec) = codec {
                println!("codec: {codec}");
            }
            if let Some(adapter) = adapter {
                println!(
                    "adapter: {:04x}:{:04x} {}",
                    adapter.vendor_id, adapter.device_id, adapter.name
                );
            }
            if let Some(replay_bytes) = replay_bytes {
                println!("replay: {replay_bytes} bytes");
            }
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

#[cfg(target_os = "linux")]
fn show_clip_saved_feedback(path: &Path) {
    let detail = clip_saved_detail(path);

    spawn_quiet(
        "notify-send",
        &[
            "--app-name=Wreath",
            "--icon=io.github.mika2go.Wreath-symbolic",
            "--urgency=normal",
            "--expire-time=3000",
            "Clip taken",
            &detail,
        ],
    );
    play_clip_saved_sound();
}

#[cfg(target_os = "linux")]
fn clip_saved_detail(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("Saved {name}"))
        .unwrap_or_else(|| "Replay saved successfully".to_owned())
}

#[cfg(target_os = "linux")]
fn spawn_quiet(program: &str, arguments: &[&str]) {
    let _ = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(target_os = "linux")]
fn play_clip_saved_sound() {
    let Ok(mut player) = Command::new("paplay")
        .args([
            "--raw",
            "--format=s16le",
            "--rate=48000",
            "--channels=1",
            "--volume",
            SOUND_PLAYBACK_VOLUME,
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn doctor() -> Result<(), String> {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).map_err(|error| error.to_string())?;
    println!("config  ok · {}", paths.config_file.display());
    let monitors = display::monitors().map_err(|error| error.to_string())?;
    println!("display  ok · {} capture target(s)", monitors.len());
    let monitor = display::resolve_monitor(&monitors, config.capture.monitor.as_deref())
        .ok_or_else(|| "no active monitor found".to_owned())?;
    println!(
        "capture  ok · {} · {}x{} @ {:.0} Hz",
        monitor.name, monitor.width, monitor.height, monitor.refresh_rate
    );
    println!(
        "buffer   ok · about {} MB for {} seconds",
        ReplaySpec::from_config(&config, monitor).estimated_buffer_megabytes(),
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
    println!("engine   ok · gpu-screen-recorder · AMD/Intel/NVIDIA detection");
    let missing_feedback = ["notify-send", "paplay", "pactl"]
        .into_iter()
        .filter(|command| !command_available(command))
        .collect::<Vec<_>>();
    if !missing_feedback.is_empty() {
        return Err(format!(
            "missing desktop feedback command(s): {}; install libnotify and libpulse",
            missing_feedback.join(", ")
        ));
    }
    println!("feedback ok · notification and quiet confirmation sound");
    println!("shortcut    · {}", shortcuts::backend());
    println!("network  blocked by packaged systemd user service");
    Ok(())
}

#[cfg(target_os = "linux")]
fn command_available(command: &str) -> bool {
    env::var_os("PATH")
        .map(|path| {
            env::split_paths(&path)
                .map(|directory| directory.join(command))
                .any(|candidate| candidate.is_file())
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
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
            let monitors = display::monitors().map_err(|error| error.to_string())?;
            let monitor = monitors
                .iter()
                .find(|monitor| monitor.name == *value || monitor.description == *value)
                .ok_or_else(|| format!("unknown monitor `{value}`; run `wreathctl monitors`"))?;
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
        "desktop-gain" => {
            config.audio.desktop_gain_percent = parse_number(value, "desktop-gain")?;
        }
        "microphone-gain" => {
            config.audio.microphone_gain_percent = parse_number(value, "microphone-gain")?;
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
                "unknown setting `{setting}`; use monitor, hotkey, duration, fps, quality, codec, desktop-gain, microphone-gain, or output"
            ));
        }
    }
    config.save(&paths).map_err(|error| error.to_string())?;
    if setting == "hotkey" {
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        shortcuts::replace(Some(&previous_hotkey), &config.hotkey, &executable)
            .map_err(|error| error.to_string())?;
    }
    let _ = send(&paths, &Request::Reload);
    println!("saved {setting} in {}", paths.config_file.display());
    Ok(())
}

#[cfg(target_os = "linux")]
fn parse_number<T>(value: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("{name} must be a number"))
}

#[cfg(target_os = "linux")]
fn send(paths: &AppPaths, request: &Request) -> Result<Response, String> {
    let mut stream = match UnixStream::connect(paths.socket_file()) {
        Ok(stream) => stream,
        Err(first_error) if matches!(request, Request::Save) => {
            start_daemon().map_err(|start_error| {
                format!(
                    "cannot connect to {}: {first_error}; could not start recorder: {start_error}",
                    paths.socket_file().display()
                )
            })?;
            let deadline = Instant::now() + DAEMON_START_TIMEOUT;
            loop {
                match UnixStream::connect(paths.socket_file()) {
                    Ok(stream) => break stream,
                    Err(_) if Instant::now() < deadline => {
                        thread::sleep(DAEMON_CONNECT_INTERVAL);
                    }
                    Err(error) => {
                        return Err(format!(
                            "recorder started but cannot connect to {} after {}s: {error}",
                            paths.socket_file().display(),
                            DAEMON_START_TIMEOUT.as_secs()
                        ));
                    }
                }
            }
        }
        Err(error) => {
            return Err(format!(
                "cannot connect to {}: {error}",
                paths.socket_file().display()
            ));
        }
    };
    ipc::write_request(&mut stream, request).map_err(|error| error.to_string())?;
    ipc::read_response(&mut BufReader::new(stream)).map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn start_daemon() -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(["--user", "start", "wreathd.service"])
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

#[cfg(target_os = "linux")]
fn print_help() {
    println!(
        "wreathctl <command>\n\n\
         commands:\n  monitors  list available capture targets\n  status    show daemon state\n  \
         save      save the replay buffer\n  pause     pause capture\n  resume    resume capture\n  \
         reload    reload local configuration\n  bind      register or explain the desktop hotkey\n  \
         sound     preview the clip confirmation sound\n  \
         cut       cut a saved clip down to one span\n  \
         config    show or change local settings\n  doctor    verify the local runtime\n  \
         shutdown  stop the daemon\n\n\
         examples:\n  wreathctl config monitor DP-1\n  wreathctl config hotkey SUPER+SHIFT+R\n  \
         wreathctl config duration 30\n  wreathctl config fps 60\n  \
         wreathctl config codec av1\n  \
         wreathctl cut ~/Videos/Wreath/clip.mp4 8 20 --name \"Best bit\""
    );
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn saved_clip_feedback_uses_the_output_filename() {
        let path = Path::new("/tmp/Wreath Replay 2026-07-29.mp4");

        assert_eq!(
            clip_saved_detail(path),
            "Saved Wreath Replay 2026-07-29.mp4"
        );
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
