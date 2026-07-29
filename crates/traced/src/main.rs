use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::ExitCode;

use trace_core::config::Config;
use trace_core::display;
use trace_core::engine::{GpuScreenRecorder, ReplaySpec};
use trace_core::ipc::{DaemonState, Request, Response};
use trace_core::paths::AppPaths;

struct Daemon {
    paths: AppPaths,
    config: Config,
    state: DaemonState,
    monitor: Option<String>,
    replay: Option<ReplaySpec>,
    recorder: Option<GpuScreenRecorder>,
    last_error: Option<String>,
    shutdown: bool,
}

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("traced: {error}");
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
    let monitors = display::monitors().map_err(|error| error.to_string())?;
    let selected_monitor =
        display::resolve_monitor(&monitors, config.capture.monitor.as_deref()).cloned();
    let monitor = selected_monitor
        .as_ref()
        .map(|monitor| monitor.name.clone());
    let replay = selected_monitor
        .as_ref()
        .map(|monitor| ReplaySpec::from_config(&config, monitor));

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
        replay,
        recorder: None,
        last_error: None,
        shutdown: false,
    };
    daemon.start_capture();
    eprintln!("traced: ready on {}", daemon.paths.socket_file.display());

    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = daemon.handle(stream) {
                    eprintln!("traced: {error}");
                }
            }
            Err(error) => eprintln!("traced: socket error: {error}"),
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
            Request::Status => self.status(),
            Request::Save => self.save_replay(),
            Request::Pause => self.pause_capture(),
            Request::Resume => self.resume_capture(),
            Request::Reload => self.reload(),
            Request::Shutdown => {
                self.stop_capture();
                self.shutdown = true;
                Response::Ok
            }
        }
    }

    fn start_capture(&mut self) {
        let Some(spec) = self.replay.as_ref() else {
            self.fail("no active Hyprland monitor found".into());
            return;
        };
        match GpuScreenRecorder::start(spec) {
            Ok(recorder) => {
                eprintln!(
                    "traced: recording {} at {} fps (estimated buffer {} MiB)",
                    spec.monitor,
                    spec.frames_per_second,
                    spec.estimated_buffer_megabytes()
                );
                self.recorder = Some(recorder);
                self.state = DaemonState::Recording;
                self.last_error = None;
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn status(&mut self) -> Response {
        if let Some(recorder) = self.recorder.as_mut()
            && let Err(error) = recorder.is_running()
        {
            self.fail(error.to_string());
        }
        Response::Status {
            state: self.state,
            monitor: self.monitor.clone(),
            buffered_seconds: if self.recorder.is_some() {
                self.config.capture.duration_seconds
            } else {
                0
            },
            error: self.last_error.clone(),
        }
    }

    fn save_replay(&mut self) -> Response {
        let Some(recorder) = self.recorder.as_mut() else {
            return Response::Error {
                message: self
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "recorder is not running".into()),
            };
        };
        match recorder.save() {
            Ok(path) => Response::Saved { path },
            Err(error) => {
                let message = error.to_string();
                self.fail(message.clone());
                Response::Error { message }
            }
        }
    }

    fn stop_capture(&mut self) {
        if let Some(mut recorder) = self.recorder.take()
            && let Err(error) = recorder.stop()
        {
            eprintln!("traced: {error}");
        }
    }

    fn pause_capture(&mut self) -> Response {
        self.stop_capture();
        self.state = DaemonState::Paused;
        self.last_error = None;
        Response::Ok
    }

    fn resume_capture(&mut self) -> Response {
        if self.recorder.is_some() {
            return Response::Ok;
        }
        self.start_capture();
        match self.last_error.clone() {
            Some(message) => Response::Error { message },
            None => Response::Ok,
        }
    }

    fn reload(&mut self) -> Response {
        let was_recording = self.recorder.is_some();
        self.stop_capture();
        let config = match Config::load(&self.paths) {
            Ok(config) => config,
            Err(error) => {
                self.fail(error.to_string());
                return Response::Error {
                    message: error.to_string(),
                };
            }
        };
        let monitors = match display::monitors() {
            Ok(monitors) => monitors,
            Err(error) => {
                self.fail(error.to_string());
                return Response::Error {
                    message: error.to_string(),
                };
            }
        };
        let selected =
            display::resolve_monitor(&monitors, config.capture.monitor.as_deref()).cloned();
        self.monitor = selected.as_ref().map(|monitor| monitor.name.clone());
        self.replay = selected
            .as_ref()
            .map(|monitor| ReplaySpec::from_config(&config, monitor));
        self.config = config;
        if was_recording {
            self.start_capture();
        } else {
            self.state = DaemonState::Paused;
        }
        match self.last_error.clone() {
            Some(message) => Response::Error { message },
            None => Response::Ok,
        }
    }

    fn fail(&mut self, message: String) {
        eprintln!("traced: {message}");
        self.recorder = None;
        self.state = DaemonState::Error;
        self.last_error = Some(message);
    }
}
