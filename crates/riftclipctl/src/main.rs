use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use riftclip_core::hyprland;
use riftclip_core::ipc::{Request, Response};
use riftclip_core::paths::AppPaths;
use riftclip_core::{config::Codec, config::Config, config::HotkeyConfig};
use riftclip_core::{engine, hyprland::resolve_monitor};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("riftclipctl: {message}");
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
        other => return Err(format!("unknown command `{other}`; try `riftclipctl help`")),
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
        Response::Saved { path } => println!("{}", path.display()),
        Response::Ok => {}
        Response::Error { message } => return Err(message),
    }
    Ok(())
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
    if !engine::recorder_available() {
        return Err(
            "gpu-screen-recorder is missing; run `sudo pacman -S gpu-screen-recorder`".into(),
        );
    }
    println!("engine   ok · gpu-screen-recorder");
    println!("network  blocked by packaged systemd user service");
    Ok(())
}

fn configure(arguments: &[String]) -> Result<(), String> {
    let paths = AppPaths::discover();
    let mut config = Config::load(&paths).map_err(|error| error.to_string())?;
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
                .ok_or_else(|| format!("unknown monitor `{value}`; run `riftclipctl monitors`"))?;
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
        hyprland::install_replay_bind(&config.hotkey, &executable)
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
    let mut stream = UnixStream::connect(&paths.socket_file)
        .map_err(|error| format!("cannot connect to {}: {error}", paths.socket_file.display()))?;
    serde_json::to_writer(&mut stream, request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&line).map_err(|error| format!("invalid daemon response: {error}"))
}

fn print_help() {
    println!(
        "riftclipctl <command>\n\n\
         commands:\n  monitors  list Hyprland monitors\n  status    show daemon state\n  \
         save      save the replay buffer\n  pause     pause capture\n  resume    resume capture\n  \
         reload    reload local configuration\n  bind      register configured Hyprland hotkey\n  \
         config    show or change local settings\n  doctor    verify the local runtime\n  \
         shutdown  stop the daemon\n\n\
         examples:\n  riftclipctl config monitor DP-1\n  riftclipctl config hotkey SUPER+SHIFT+R\n  \
         riftclipctl config duration 30\n  riftclipctl config fps 60\n  \
         riftclipctl config codec av1"
    );
}
