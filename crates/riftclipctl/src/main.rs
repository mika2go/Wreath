use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use riftclip_core::hyprland;
use riftclip_core::ipc::{Request, Response};
use riftclip_core::paths::AppPaths;

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
    let command = env::args().nth(1).unwrap_or_else(|| "status".into());
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

    let request = match command.as_str() {
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
         reload    reload local configuration\n  shutdown  stop the daemon"
    );
}
