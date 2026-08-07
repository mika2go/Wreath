use std::io::BufReader;

use wreath_core::config::Config;
use wreath_core::ipc::{self, DaemonState, Request, Response};
use wreath_core::paths::AppPaths;
use wreath_windows::control::NamedPipeServer;
use wreath_windows::hotkey::HotkeyListener;

pub fn run() -> Result<(), String> {
    let paths = AppPaths::discover();
    let config = Config::load(&paths).map_err(|error| error.to_string())?;
    if !paths.config_file.exists() {
        config.save(&paths).map_err(|error| error.to_string())?;
    }
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
            Request::Status => Response::Status {
                state: DaemonState::Error,
                monitor: None,
                buffered_seconds: 0,
                error: Some("Windows capture backend is not available yet".into()),
            },
            Request::Shutdown => {
                shutdown = true;
                Response::Ok
            }
            Request::Save | Request::Pause | Request::Resume | Request::Reload => Response::Error {
                message: "Windows capture backend is not available yet".into(),
            },
        };
        ipc::write_response(&mut connection, &response).map_err(|error| error.to_string())?;
    }
    Ok(())
}
