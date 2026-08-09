use std::fmt;
use std::io;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

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
    send_request_with_timeout(pipe_name, request, Duration::from_secs(2))
}

/// Connects to the daemon without dropping a request just because the single
/// named-pipe instance is serving the tray or another UI client. Windows
/// reports that short race as `ERROR_PIPE_BUSY`; waiting and reopening is the
/// expected client-side recovery path.
#[cfg(target_os = "windows")]
pub fn send_request_with_timeout(
    pipe_name: &str,
    request: &wreath_core::ipc::Request,
    connect_timeout: Duration,
) -> Result<wreath_core::ipc::Response, ControlError> {
    use std::fs::OpenOptions;
    use std::io::BufReader;

    let deadline = Instant::now() + connect_timeout;
    let mut pipe = loop {
        match OpenOptions::new().read(true).write(true).open(pipe_name) {
            Ok(pipe) => break pipe,
            Err(error) if retryable_pipe_open_error(&error) && Instant::now() < deadline => {
                wait_for_pipe(pipe_name, deadline)?;
            }
            Err(error) => return Err(ControlError::Io(error)),
        }
    };
    wreath_core::ipc::write_request(&mut pipe, request).map_err(ControlError::Protocol)?;
    wreath_core::ipc::read_response(&mut BufReader::new(pipe)).map_err(ControlError::Protocol)
}

#[cfg(target_os = "windows")]
fn retryable_pipe_open_error(error: &io::Error) -> bool {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};

    matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_PIPE_BUSY.0 as i32 || code == ERROR_FILE_NOT_FOUND.0 as i32
    )
}

#[cfg(target_os = "windows")]
fn wait_for_pipe(pipe_name: &str, deadline: Instant) -> Result<(), ControlError> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SEM_TIMEOUT};
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::core::PCWSTR;

    let name = pipe_name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let remaining = deadline.saturating_duration_since(Instant::now());
    let wait_ms = remaining.as_millis().clamp(1, 250) as u32;
    if unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), wait_ms) }.as_bool() {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    let transient = matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_SEM_TIMEOUT.0 as i32 || code == ERROR_FILE_NOT_FOUND.0 as i32
    );
    if transient && Instant::now() < deadline {
        // A daemon that is starting or recreating its sole pipe instance can
        // briefly return FILE_NOT_FOUND between two accepted clients.
        std::thread::sleep(Duration::from_millis(10));
        Ok(())
    } else {
        Err(ControlError::Io(error))
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn a_busy_pipe_waits_for_the_next_server_instance() {
        use std::io::BufReader;
        use std::sync::mpsc;

        use wreath_core::ipc::{Request, Response};

        let pipe_name = format!(
            r"\\.\pipe\wreath-control-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let server = NamedPipeServer::new(&pipe_name).unwrap();
        let (first_request_sender, first_request_receiver) = mpsc::sync_channel(1);
        let server_thread = std::thread::spawn(move || {
            let mut first = server.accept().unwrap();
            let request =
                wreath_core::ipc::read_request(&mut BufReader::new(first.try_clone().unwrap()))
                    .unwrap();
            assert_eq!(request, Request::Status);
            first_request_sender.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            wreath_core::ipc::write_response(&mut first, &Response::Ok).unwrap();
            drop(first);

            let mut second = server.accept().unwrap();
            let request =
                wreath_core::ipc::read_request(&mut BufReader::new(second.try_clone().unwrap()))
                    .unwrap();
            assert_eq!(request, Request::Save);
            wreath_core::ipc::write_response(&mut second, &Response::Ok).unwrap();
        });

        let first_pipe = pipe_name.clone();
        let first_client = std::thread::spawn(move || {
            send_request_with_timeout(&first_pipe, &Request::Status, Duration::from_secs(2))
        });
        first_request_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();

        let second =
            send_request_with_timeout(&pipe_name, &Request::Save, Duration::from_secs(2)).unwrap();

        assert_eq!(second, Response::Ok);
        assert_eq!(first_client.join().unwrap().unwrap(), Response::Ok);
        server_thread.join().unwrap();
    }
}
