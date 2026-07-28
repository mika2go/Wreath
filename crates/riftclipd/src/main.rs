use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::ExitCode;

use riftclip_core::config::Config;
use riftclip_core::hyprland;
use riftclip_core::ipc::{DaemonState, Request, Response};
use riftclip_core::paths::AppPaths;

struct Daemon {
    paths: AppPaths,
    config: Config,
    state: DaemonState,
    monitor: Option<String>,
    shutdown: bool,
}

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("riftclipd: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).map_err(|error| error.to_string())?;
    if !paths.config_file.exists() {
        config.save(&paths).map_err(|error| error.to_string())?;
    }
    let monitors = hyprland::monitors().map_err(|error| error.to_string())?;
    let monitor = hyprland::resolve_monitor(&monitors, config.capture.monitor.as_deref())
        .map(|monitor| monitor.name.clone());

    if let Some(parent) = paths.socket_file.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if paths.socket_file.exists() {
        fs::remove_file(&paths.socket_file).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&paths.socket_file).map_err(|error| error.to_string())?;
    fs::set_permissions(&paths.socket_file, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    let mut daemon = Daemon {
        paths,
        config,
        state: DaemonState::Starting,
        monitor,
        shutdown: false,
    };
    daemon.state = DaemonState::Paused;
    eprintln!(
        "riftclipd: ready on {} (capture backend follows in milestone 2)",
        daemon.paths.socket_file.display()
    );

    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = daemon.handle(stream) {
                    eprintln!("riftclipd: {error}");
                }
            }
            Err(error) => eprintln!("riftclipd: socket error: {error}"),
        }
        if daemon.shutdown {
            break;
        }
    }
    let _ = fs::remove_file(&daemon.paths.socket_file);
    Ok(())
}

impl Daemon {
    fn handle(&mut self, mut stream: UnixStream) -> Result<(), String> {
        let mut line = String::new();
        BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("socket clone failed: {error}"))?,
        )
        .read_line(&mut line)
        .map_err(|error| format!("socket read failed: {error}"))?;
        let request: Request =
            serde_json::from_str(&line).map_err(|error| format!("invalid request: {error}"))?;
        let response = self.respond(request);
        serde_json::to_writer(&mut stream, &response)
            .map_err(|error| format!("socket write failed: {error}"))?;
        stream
            .write_all(b"\n")
            .map_err(|error| format!("socket write failed: {error}"))
    }

    fn respond(&mut self, request: Request) -> Response {
        match request {
            Request::Status => Response::Status {
                state: self.state,
                monitor: self.monitor.clone(),
                buffered_seconds: 0,
            },
            Request::Save => Response::Error {
                message: "capture backend is not active yet".into(),
            },
            Request::Pause => {
                self.state = DaemonState::Paused;
                Response::Ok
            }
            Request::Resume => {
                self.state = DaemonState::Recording;
                Response::Ok
            }
            Request::Reload => match Config::load(&self.paths) {
                Ok(config) => {
                    self.config = config;
                    Response::Ok
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            },
            Request::Shutdown => {
                self.shutdown = true;
                Response::Ok
            }
        }
    }
}
