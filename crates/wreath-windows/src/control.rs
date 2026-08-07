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
pub struct NamedPipeServer {
    name: Vec<u16>,
}

#[cfg(target_os = "windows")]
impl NamedPipeServer {
    pub fn new(pipe_name: &str) -> Result<Self, ControlError> {
        if pipe_name.encode_utf16().any(|unit| unit == 0) {
            return Err(ControlError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "named pipe contains a null character",
            )));
        }
        Ok(Self {
            name: pipe_name.encode_utf16().chain(Some(0)).collect(),
        })
    }

    pub fn accept(&self) -> Result<std::fs::File, ControlError> {
        use std::os::windows::io::FromRawHandle;

        use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        };
        use windows::core::{HRESULT, PCWSTR};

        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(self.name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4_096,
                4_096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(ControlError::Io(io::Error::last_os_error()));
        }
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        if let Err(error) = connected
            && error.code() != HRESULT::from_win32(ERROR_PIPE_CONNECTED.0)
        {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
            return Err(ControlError::Io(io::Error::from_raw_os_error(
                error.code().0,
            )));
        }
        Ok(unsafe { std::fs::File::from_raw_handle(handle.0) })
    }
}

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
