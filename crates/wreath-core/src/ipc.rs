use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Request {
    Status,
    Save,
    Pause,
    Resume,
    Reload,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum Response {
    Status {
        state: DaemonState,
        monitor: Option<String>,
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
