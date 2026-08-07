use std::fmt;
use std::io;

use wreath_core::ipc::IpcError;

#[derive(Debug)]
pub enum ControlError {
    Io(io::Error),
    Protocol(IpcError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot open Wreath named pipe: {error}"),
            Self::Protocol(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ControlError {}

#[cfg(target_os = "windows")]
pub fn send_request(
    pipe_name: &str,
    request: &wreath_core::ipc::Request,
) -> Result<wreath_core::ipc::Response, ControlError> {
    use std::fs::OpenOptions;
    use std::io::BufReader;

    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)
        .map_err(ControlError::Io)?;
    wreath_core::ipc::write_request(&mut pipe, request).map_err(ControlError::Protocol)?;
    wreath_core::ipc::read_response(&mut BufReader::new(pipe)).map_err(ControlError::Protocol)
}
