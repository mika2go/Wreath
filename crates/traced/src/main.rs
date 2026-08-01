use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use trace_core::config::Config;
use trace_core::display;
use trace_core::engine::{GpuScreenRecorder, ReplaySpec};
use trace_core::ipc::{DaemonState, Request, Response};
use trace_core::paths::AppPaths;

const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(250);
const RECORDER_READY_DELAY: Duration = Duration::from_millis(750);

struct Daemon {
    paths: AppPaths,
    config: Config,
    state: DaemonState,
    monitor: Option<String>,
    replay: Option<ReplaySpec>,
    recorder: Option<GpuScreenRecorder>,
    capture_requested: bool,
    capture_started_at: Option<Instant>,
    restart_attempts: u32,
    next_restart_at: Option<Instant>,
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
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    fs::set_permissions(&paths.socket_file, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;

    let mut daemon = Daemon {
        paths,
        config,
        state: DaemonState::Starting,
        monitor,
        replay,
        recorder: None,
        capture_requested: true,
        capture_started_at: None,
        restart_attempts: 0,
        next_restart_at: None,
        last_error: None,
        shutdown: false,
    };
    daemon.start_capture();
    eprintln!("traced: ready on {}", daemon.paths.socket_file.display());

    while !daemon.shutdown {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = daemon.handle(stream) {
                        eprintln!("traced: {error}");
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    eprintln!("traced: socket error: {error}");
                    break;
                }
            }
            if daemon.shutdown {
                break;
            }
        }
        daemon.maintain_capture();
        if !daemon.shutdown {
            thread::sleep(HEALTH_CHECK_INTERVAL);
        }
    }
    let _ = fs::remove_file(&daemon.paths.socket_file);
    Ok(())
}

impl Daemon {
    fn handle(&mut self, mut stream: UnixStream) -> Result<(), String> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| format!("socket timeout setup failed: {error}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| format!("socket timeout setup failed: {error}"))?;
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
                self.capture_requested = false;
                self.stop_capture();
                self.shutdown = true;
                Response::Ok
            }
        }
    }

    fn start_capture(&mut self) {
        if !self.capture_requested {
            return;
        }
        self.state = DaemonState::Starting;
        self.next_restart_at = None;
        let Some(spec) = self.replay.as_ref() else {
            self.fail("no active capture target found".into());
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
                self.capture_started_at = Some(Instant::now());
                self.restart_attempts = 0;
                self.next_restart_at = None;
                self.state = DaemonState::Recording;
                self.last_error = None;
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    fn status(&mut self) -> Response {
        self.check_recorder();
        Response::Status {
            state: self.state,
            monitor: self.monitor.clone(),
            buffered_seconds: self
                .capture_started_at
                .map(|started| {
                    started
                        .elapsed()
                        .as_secs()
                        .min(u64::from(self.config.capture.duration_seconds))
                        as u16
                })
                .unwrap_or(0),
            error: self.last_error.clone(),
        }
    }

    fn save_replay(&mut self) -> Response {
        self.check_recorder();
        if self.recorder.is_none() && self.capture_requested {
            self.restart_attempts = 0;
            self.next_restart_at = None;
            self.start_capture();
        }
        if let Some(started) = self.capture_started_at {
            let elapsed = started.elapsed();
            if elapsed < RECORDER_READY_DELAY {
                thread::sleep(RECORDER_READY_DELAY - elapsed);
                self.check_recorder();
            }
        }
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
        self.capture_started_at = None;
        if let Some(mut recorder) = self.recorder.take()
            && let Err(error) = recorder.stop()
        {
            eprintln!("traced: {error}");
        }
    }

    fn pause_capture(&mut self) -> Response {
        self.capture_requested = false;
        self.next_restart_at = None;
        self.stop_capture();
        self.state = DaemonState::Paused;
        self.last_error = None;
        Response::Ok
    }

    fn resume_capture(&mut self) -> Response {
        if self.recorder.is_some() {
            return Response::Ok;
        }
        self.capture_requested = true;
        self.restart_attempts = 0;
        self.next_restart_at = None;
        self.start_capture();
        match self.last_error.clone() {
            Some(message) => Response::Error { message },
            None => Response::Ok,
        }
    }

    fn reload(&mut self) -> Response {
        let should_record = self.capture_requested;
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
        if should_record {
            self.restart_attempts = 0;
            self.next_restart_at = None;
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
        self.capture_started_at = None;
        self.state = DaemonState::Error;
        self.last_error = Some(message);
        if self.capture_requested {
            let delay = restart_delay(self.restart_attempts);
            self.restart_attempts = self.restart_attempts.saturating_add(1);
            self.next_restart_at = Some(Instant::now() + delay);
            eprintln!("traced: retrying capture in {}s", delay.as_secs());
        }
    }

    fn check_recorder(&mut self) {
        let error = self
            .recorder
            .as_mut()
            .and_then(|recorder| recorder.is_running().err());
        if let Some(error) = error {
            self.fail(error.to_string());
        }
    }

    fn maintain_capture(&mut self) {
        self.check_recorder();
        if self.capture_requested
            && self.recorder.is_none()
            && self
                .next_restart_at
                .is_none_or(|restart_at| Instant::now() >= restart_at)
        {
            self.start_capture();
        }
    }
}

fn restart_delay(attempt: u32) -> Duration {
    Duration::from_secs((1_u64 << attempt.min(5)).min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_restart_delay_is_fast_then_bounded() {
        assert_eq!(restart_delay(0), Duration::from_secs(1));
        assert_eq!(restart_delay(1), Duration::from_secs(2));
        assert_eq!(restart_delay(5), Duration::from_secs(30));
        assert_eq!(restart_delay(100), Duration::from_secs(30));
    }
}
