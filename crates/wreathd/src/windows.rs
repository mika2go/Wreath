use std::io::BufReader;

use wreath_core::config::Config;
use wreath_core::ipc::{self, DaemonState, GraphicsAdapter, Request, Response};
use wreath_core::paths::AppPaths;
use wreath_windows::control::NamedPipeServer;
use wreath_windows::hotkey::HotkeyListener;
use wreath_windows::pipeline::{PipelineRunState, ReplayPipeline};

pub fn run() -> Result<(), String> {
    let paths = AppPaths::discover();
    let mut config = Config::load(&paths).map_err(|error| error.to_string())?;
    let needs_initial_save = !paths.config_file.exists();
    let migrated_hotkey = wreath_windows::hotkey::migrate_legacy_windows_hotkey(&mut config.hotkey);
    if needs_initial_save || migrated_hotkey {
        config.save(&paths).map_err(|error| error.to_string())?;
    }
    let mut pipeline = ReplayPipeline::spawn(config.clone()).map_err(|error| error.to_string())?;
    let server = NamedPipeServer::new(paths.pipe_name()).map_err(|error| error.to_string())?;
    let pipe_name = paths.pipe_name().to_owned();
    let _hotkey = HotkeyListener::spawn(1, &config.hotkey, move || {
        if let Err(error) = wreath_windows::control::send_request(&pipe_name, &Request::Save) {
            eprintln!("wreathd: hotkey save failed: {error}");
        }
    })
    .map_err(|error| error.to_string())?;
    let mut shutdown = false;
    while !shutdown {
        let mut connection = server.accept().map_err(|error| error.to_string())?;
        let request = ipc::read_request(&mut BufReader::new(
            connection
                .try_clone()
                .map_err(|error| format!("cannot clone named pipe: {error}"))?,
        ))
        .map_err(|error| error.to_string())?;
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
            Request::Save => pipeline
                .save()
                .map(|path| Response::Saved { path })
                .unwrap_or_else(|error| Response::Error {
                    message: error.to_string(),
                }),
            Request::Reload => reload(&paths, &mut config, &mut pipeline, &_hotkey),
        };
        ipc::write_response(&mut connection, &response).map_err(|error| error.to_string())?;
    }
    Ok(())
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
