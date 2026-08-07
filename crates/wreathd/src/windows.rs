use std::io::BufReader;

use wreath_core::config::Config;
use wreath_core::ipc::{self, DaemonState, Request, Response};
use wreath_core::paths::AppPaths;
use wreath_windows::control::NamedPipeServer;
use wreath_windows::hotkey::HotkeyListener;
use wreath_windows::pipeline::{PipelineRunState, ReplayPipeline};

pub fn run() -> Result<(), String> {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).map_err(|error| error.to_string())?;
    if !paths.config_file.exists() {
        config.save(&paths).map_err(|error| error.to_string())?;
    }
    let pipeline = ReplayPipeline::spawn(config.clone()).map_err(|error| error.to_string())?;
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
            Request::Save => Response::Error {
                message: "Windows MP4 muxing is the next build step".into(),
            },
            Request::Reload => Response::Error {
                message: "restart wreathd to reload Windows capture settings".into(),
            },
        };
        ipc::write_response(&mut connection, &response).map_err(|error| error.to_string())?;
    }
    Ok(())
}
