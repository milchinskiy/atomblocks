use std::{fmt::Debug, sync::mpsc::SendError};

pub enum AtomBlocksError {
    IOError(std::io::Error),
    Config(String),
    SendReceived(SendError<(usize, String)>),
    X11Reply(x11rb::errors::ReplyError),
    X11Connect(x11rb::errors::ConnectError),
    X11Connection(x11rb::errors::ConnectionError),
}

impl AtomBlocksError {
    pub fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IOError(e) => Some(e),
            Self::SendReceived(e) => Some(e),
            Self::X11Reply(e) => Some(e),
            Self::X11Connect(e) => Some(e),
            Self::X11Connection(e) => Some(e),
            Self::Config(_) => None,
        }
    }
}

impl From<std::io::Error> for AtomBlocksError {
    fn from(value: std::io::Error) -> Self {
        Self::IOError(value)
    }
}

impl From<toml::de::Error> for AtomBlocksError {
    fn from(value: toml::de::Error) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<SendError<(usize, String)>> for AtomBlocksError {
    fn from(value: SendError<(usize, String)>) -> Self {
        Self::SendReceived(value)
    }
}

impl From<x11rb::errors::ConnectError> for AtomBlocksError {
    fn from(value: x11rb::errors::ConnectError) -> Self {
        Self::X11Connect(value)
    }
}

impl From<x11rb::errors::ConnectionError> for AtomBlocksError {
    fn from(value: x11rb::errors::ConnectionError) -> Self {
        Self::X11Connection(value)
    }
}

impl From<x11rb::errors::ReplyError> for AtomBlocksError {
    fn from(value: x11rb::errors::ReplyError) -> Self {
        Self::X11Reply(value)
    }
}

impl Debug for AtomBlocksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IOError(e) => Debug::fmt(&e, f),
            Self::Config(e) => Debug::fmt(&e, f),
            Self::X11Reply(e) => Debug::fmt(&e, f),
            Self::X11Connect(e) => Debug::fmt(&e, f),
            Self::X11Connection(e) => Debug::fmt(&e, f),
            Self::SendReceived(e) => Debug::fmt(&e, f),
        }
    }
}

impl std::error::Error for AtomBlocksError {}

impl std::fmt::Display for AtomBlocksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IOError(e) => Debug::fmt(&e, f),
            Self::Config(e) => Debug::fmt(&e, f),
            Self::X11Reply(e) => Debug::fmt(&e, f),
            Self::X11Connect(e) => Debug::fmt(&e, f),
            Self::X11Connection(e) => Debug::fmt(&e, f),
            Self::SendReceived(e) => Debug::fmt(&e, f),
        }
    }
}
