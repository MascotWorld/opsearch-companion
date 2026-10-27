use std::path::PathBuf;
use std::time::SystemTime;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("the request could not be completed (HTTP {0})")]
    HttpStatus(u16),
    #[error("network request failed")]
    Network,
    #[error("the companion worker is no longer available")]
    WorkerStopped,
    #[error("Steam integration is available on Windows only")]
    WindowsOnly,
}

pub type CoreResult<T> = Result<T, CoreError>;

pub enum Command {
    Pair {
        code: String,
        server: String,
    },
    Start,
    Stop,
    Inspect {
        path: PathBuf,
        owner: Option<String>,
    },
    Shutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Idle,
    SignedOut,
    NeedsPairing { steam_id64: String },
    Watching { steam_id64: String },
    Pairing,
    Stopped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub steamid64: String,
    pub playtime: i64,
    pub app_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Roster {
    pub owner_steamid64: String,
    pub owner_playtime: i64,
    pub observed_at: SystemTime,
    pub players: Vec<Player>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Status(Status),
    PairComplete { steam_id64: String },
    Roster(Roster),
    Error(String),
}
