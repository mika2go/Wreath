use std::io::BufReader;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use wreath_core::config::Config;
use wreath_core::ipc::{self, DaemonState, GraphicsAdapter, Request, Response};
use wreath_core::paths::AppPaths;
use wreath_windows::control::{DeadlineReader, NamedPipeServer, SingleInstance};
use wreath_windows::hotkey::{HotkeyListener, SaveGuard};
use wreath_windows::pipeline::{PipelineRunState, ReplayPipeline};

/// A client writes its request the moment it is connected, so anything slower
/// than this is a client that died mid-request. Waiting on it would block
/// every other client, including the hotkey: while a connection is being
/// served there is no free pipe instance for anyone else.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Long enough to cover the slowest answer the daemon can give, so a press is
/// only ever rejected while a save really is running.
const SAVE_GUARD_TIMEOUT: Duration = Duration::from_secs(60);
const HOTKEY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run() -> Result<(), String> {
    let Some(_single_instance) = claim_single_instance()? else {
        // A second daemon would fight the running one over the control pipe,
        // the shortcut, and the capture device. Leaving quietly is what the
        // tray's recovery path expects.
        return Ok(());
    };
    let paths = AppPaths::discover();
    let mut config = Config::load(&paths).map_err(|error| error.to_string())?;
    let needs_initial_save = !paths.config_file.exists();
    let migrated_hotkey = wreath_windows::hotkey::migrate_legacy_windows_hotkey(&mut config.hotkey);
    if needs_initial_save || migrated_hotkey {
        config.save(&paths).map_err(|error| error.to_string())?;
    }
    let server = NamedPipeServer::new(paths.pipe_name()).map_err(|error| error.to_string())?;
    // Create the named-pipe instance before graphics/audio initialization. On
    // slower machines that initialization can take several seconds; clients
    // can now connect immediately and wait for the daemon instead of seeing a
    // misleading "file not found" error during normal startup.
    let (connections, handled) = listen(server)?;
    let mut pipeline = ReplayPipeline::spawn(config.clone()).map_err(|error| error.to_string())?;
    let pipe_name = paths.pipe_name().to_owned();
    let save_guard = Arc::new(SaveGuard::new(SAVE_GUARD_TIMEOUT));
    let hotkey = HotkeyListener::spawn(1, &config.hotkey, move || {
        if !save_guard.acquire(Instant::now()) {
            wreath_core::diagnostic!(
                "Wreath hotkey: replay request ignored because a save is already running"
            );
            return;
        }
        wreath_core::diagnostic!("Wreath hotkey: replay requested");
        let pipe_name = pipe_name.clone();
        let save_guard_for_worker = Arc::clone(&save_guard);
        let spawn = std::thread::Builder::new()
            .name("wreath-hotkey-save".into())
            .spawn(move || {
                let _release = SaveGuardRelease(save_guard_for_worker);
                match wreath_windows::control::send_request_with_timeout(
                    &pipe_name,
                    &Request::Save,
                    HOTKEY_CONNECT_TIMEOUT,
                ) {
                    Ok(Response::Saved { path }) => {
                        wreath_core::diagnostic!(
                            "Wreath hotkey: replay saved to {}",
                            path.display()
                        );
                        wreath_windows::feedback::broadcast_clip_saved();
                    }
                    Ok(Response::Error { message }) => {
                        wreath_core::diagnostic!("Wreath hotkey: replay save failed: {message}")
                    }
                    Err(error) => {
                        wreath_core::diagnostic!("Wreath hotkey: replay request failed: {error}")
                    }
                    Ok(response) => wreath_core::diagnostic!(
                        "Wreath hotkey: unexpected replay response: {response:?}"
                    ),
                }
            });
        if let Err(error) = spawn {
            save_guard.release();
            wreath_core::diagnostic!("Wreath hotkey: cannot start save worker: {error}");
        }
    })
    .map_err(|error| error.to_string())?;
    let mut shutdown = false;
    while !shutdown {
        let mut connection = connections
            .recv()
            .map_err(|error| format!("Windows control listener stopped: {error}"))??;
        let request = {
            let mut reader =
                BufReader::new(DeadlineReader::new(&mut connection, REQUEST_READ_TIMEOUT));
            ipc::read_request(&mut reader)
        };
        // One client that vanishes mid-request used to end the daemon, and
        // with it the replay buffer and the registered shortcut. Serving the
        // next client is always the better answer.
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                wreath_core::diagnostic!("Wreath control: ignoring an unreadable request: {error}");
                drop(connection);
                handled
                    .send(())
                    .map_err(|error| format!("Windows control listener stopped: {error}"))?;
                continue;
            }
        };
        let response = match request {
            Request::Status => {
                let status = pipeline.status();
                Response::Status {
                    state: match status.state {
                        PipelineRunState::Starting => DaemonState::Starting,
                        PipelineRunState::Recording => DaemonState::Recording,
                        PipelineRunState::Paused => DaemonState::Paused,
                        PipelineRunState::Error => DaemonState::Error,
                    },
                    monitor: status.monitor,
                    codec: status.codec.map(|codec| codec.as_str().to_owned()),
                    adapter: status.adapter.map(|adapter| GraphicsAdapter {
                        name: adapter.name,
                        vendor_id: adapter.vendor_id,
                        device_id: adapter.device_id,
                    }),
                    replay_bytes: Some(u64::try_from(status.encoded_bytes).unwrap_or(u64::MAX)),
                    buffered_seconds: status.buffered_seconds,
                    error: status.error,
                }
            }
            Request::Shutdown => {
                shutdown = true;
                Response::Ok
            }
            Request::Pause => pipeline
                .pause()
                .map(|()| Response::Ok)
                .unwrap_or_else(|error| Response::Error {
                    message: error.to_string(),
                }),
            Request::Resume => pipeline
                .resume()
                .map(|()| Response::Ok)
                .unwrap_or_else(|error| Response::Error {
                    message: error.to_string(),
                }),
            Request::SetHotkey { hotkey: new_hotkey } => {
                set_hotkey(&paths, &mut config, &hotkey, new_hotkey)
            }
            Request::Save => pipeline
                .save()
                .map(|path| Response::Saved { path })
                .unwrap_or_else(|error| Response::Error {
                    message: error.to_string(),
                }),
            Request::Reload => reload(&paths, &mut config, &mut pipeline, &hotkey),
        };
        if let Err(error) = ipc::write_response(&mut connection, &response) {
            wreath_core::diagnostic!("Wreath control: cannot answer a client: {error}");
        }
        // Windows keeps a single instance of this pipe, so the served handle
        // has to be closed before the listener is allowed to create the next
        // one. Releasing the listener first left the two racing, and a lost
        // race ended the daemon.
        drop(connection);
        handled
            .send(())
            .map_err(|error| format!("Windows control listener stopped: {error}"))?;
    }
    Ok(())
}

struct SaveGuardRelease(Arc<SaveGuard>);

impl Drop for SaveGuardRelease {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn set_hotkey(
    paths: &AppPaths,
    config: &mut Config,
    listener: &HotkeyListener,
    new_hotkey: wreath_core::config::HotkeyConfig,
) -> Response {
    if let Err(error) = wreath_windows::hotkey::validate_hotkey_choice(&new_hotkey) {
        return Response::Error {
            message: error.to_string(),
        };
    }
    if new_hotkey == config.hotkey {
        return Response::Ok;
    }
    if let Err(error) = listener.rebind(&new_hotkey) {
        return Response::Error {
            message: format!("cannot activate the new Windows shortcut: {error}"),
        };
    }

    let previous_hotkey = std::mem::replace(&mut config.hotkey, new_hotkey);
    if let Err(error) = config.save(paths) {
        config.hotkey = previous_hotkey.clone();
        let restoration = listener.rebind(&previous_hotkey);
        return Response::Error {
            message: match restoration {
                Ok(()) => format!("cannot save the new Windows shortcut: {error}"),
                Err(restore_error) => format!(
                    "cannot save the new Windows shortcut ({error}); the previous shortcut could not be restored ({restore_error})"
                ),
            },
        };
    }
    Response::Ok
}

fn claim_single_instance() -> Result<Option<SingleInstance>, String> {
    SingleInstance::claim_daemon().map_err(|error| error.to_string())
}

type AcceptedConnection = Result<std::fs::File, String>;

/// Creating the next pipe instance can fail while Windows is still tearing the
/// previous one down. That is a moment to wait out, not a reason to stop.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
/// A failure that survives this long is not transient. Ending the daemon is
/// then the right move: it releases the registered shortcut so the tray can
/// start a service that answers again.
const ACCEPT_RETRY_LIMIT: u32 = 50;

fn listen(
    server: NamedPipeServer,
) -> Result<(mpsc::Receiver<AcceptedConnection>, mpsc::SyncSender<()>), String> {
    let (connection_sender, connection_receiver) = mpsc::sync_channel(0);
    let (handled_sender, handled_receiver) = mpsc::sync_channel(0);
    std::thread::Builder::new()
        .name("wreath-control".into())
        .spawn(move || {
            let mut failures = 0;
            loop {
                match server.accept() {
                    Ok(connection) => {
                        failures = 0;
                        if connection_sender.send(Ok(connection)).is_err() {
                            return;
                        }
                        if handled_receiver.recv().is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        failures += 1;
                        if failures >= ACCEPT_RETRY_LIMIT {
                            let _ = connection_sender.send(Err(error.to_string()));
                            return;
                        }
                        wreath_core::diagnostic!(
                            "Wreath control: cannot open the control channel, retrying: {error}"
                        );
                        std::thread::sleep(ACCEPT_RETRY_DELAY);
                    }
                }
            }
        })
        .map_err(|error| format!("cannot start Windows control listener: {error}"))?;
    Ok((connection_receiver, handled_sender))
}

fn reload(
    paths: &AppPaths,
    current_config: &mut Config,
    pipeline: &mut ReplayPipeline,
    hotkey: &HotkeyListener,
) -> Response {
    let new_config = match Config::load(paths) {
        Ok(config) => config,
        Err(error) => {
            return Response::Error {
                message: error.to_string(),
            };
        }
    };
    let config_changed = &new_config != current_config;
    let pipeline_state = pipeline.status().state;
    if !config_changed && pipeline_state != PipelineRunState::Error {
        return Response::Ok;
    }

    let was_paused = pipeline_state == PipelineRunState::Paused;
    let replacement = match ReplayPipeline::spawn(new_config.clone()) {
        Ok(pipeline) => pipeline,
        Err(error) => {
            return Response::Error {
                message: format!("new Windows capture settings were rejected: {error}"),
            };
        }
    };
    if was_paused && let Err(error) = replacement.pause() {
        return Response::Error {
            message: format!("cannot preserve paused state while reloading: {error}"),
        };
    }
    if config_changed && let Err(error) = hotkey.rebind(&new_config.hotkey) {
        return Response::Error {
            message: format!("cannot activate the new Windows shortcut: {error}"),
        };
    }

    let previous = std::mem::replace(pipeline, replacement);
    *current_config = new_config;
    drop(previous);
    Response::Ok
}
