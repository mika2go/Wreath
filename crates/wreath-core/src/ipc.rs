use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::HotkeyConfig;

#[derive(Debug)]
pub enum IpcError {
    Io(io::Error),
    Json(serde_json::Error),
    Closed,
}

impl fmt::Display for IpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "control channel I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "invalid control message: {error}"),
            Self::Closed => formatter.write_str("control channel closed before a message arrived"),
        }
    }
}

impl std::error::Error for IpcError {}

impl From<io::Error> for IpcError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for IpcError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Request {
    Status,
    Save,
    Pause,
    Resume,
    SetHotkey { hotkey: HotkeyConfig },
    Reload,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphicsAdapter {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum Response {
    Status {
        state: DaemonState,
        monitor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        codec: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adapter: Option<GraphicsAdapter>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_bytes: Option<u64>,
        buffered_seconds: u16,
        error: Option<String>,
    },
    Saved {
        path: PathBuf,
    },
    Ok,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonState {
    Starting,
    Recording,
    Paused,
    Error,
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Request, IpcError> {
    read_message(reader)
}

pub fn write_request(writer: &mut impl Write, request: &Request) -> Result<(), IpcError> {
    write_message(writer, request)
}

pub fn read_response(reader: &mut impl BufRead) -> Result<Response, IpcError> {
    read_message(reader)
}

pub fn write_response(writer: &mut impl Write, response: &Response) -> Result<(), IpcError> {
    write_message(writer, response)
}

fn read_message<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, IpcError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(IpcError::Closed);
    }
    serde_json::from_str(&line).map_err(IpcError::Json)
}

fn write_message<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<(), IpcError> {
    serde_json::to_writer(&mut *writer, message)?;
    writer.write_all(b"\n").map_err(IpcError::Io)
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn request_round_trips_over_line_framing() {
        let mut bytes = Vec::new();
        write_request(&mut bytes, &Request::Save).unwrap();

        let mut reader = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_request(&mut reader).unwrap(), Request::Save);
    }

    #[test]
    fn hotkey_update_round_trips_over_line_framing() {
        let request = Request::SetHotkey {
            hotkey: HotkeyConfig::parse("CTRL+ALT+F8").unwrap(),
        };
        let mut bytes = Vec::new();
        write_request(&mut bytes, &request).unwrap();

        let mut reader = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_request(&mut reader).unwrap(), request);
    }

    #[test]
    fn response_round_trips_over_line_framing() {
        let response = Response::Status {
            state: DaemonState::Recording,
            monitor: Some("DISPLAY-1".into()),
            codec: Some("h264".into()),
            adapter: Some(GraphicsAdapter {
                name: "Example GPU".into(),
                vendor_id: 0x1002,
                device_id: 0x73bf,
            }),
            replay_bytes: Some(12_345_678),
            buffered_seconds: 30,
            error: None,
        };
        let mut bytes = Vec::new();
        write_response(&mut bytes, &response).unwrap();

        let mut reader = BufReader::new(Cursor::new(bytes));
        assert_eq!(read_response(&mut reader).unwrap(), response);
    }

    #[test]
    fn status_without_codec_remains_backward_compatible() {
        let legacy = br#"{"result":"status","state":"recording","monitor":"DISPLAY-1","buffered_seconds":30,"error":null}
"#;
        let mut reader = BufReader::new(Cursor::new(legacy));

        assert_eq!(
            read_response(&mut reader).unwrap(),
            Response::Status {
                state: DaemonState::Recording,
                monitor: Some("DISPLAY-1".into()),
                codec: None,
                adapter: None,
                replay_bytes: None,
                buffered_seconds: 30,
                error: None,
            }
        );
    }

    #[test]
    fn closed_channel_has_a_distinct_error() {
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(matches!(read_request(&mut reader), Err(IpcError::Closed)));
    }
}
