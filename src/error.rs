use std::{fmt, path::PathBuf};

#[derive(Debug)]
pub enum AtomBlocksError {
    IOError(std::io::Error),
    Config(String),
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Runtime(String),
    Interrupted(i32),
    X11Reply(x11rb::errors::ReplyError),
    X11Connect(x11rb::errors::ConnectError),
    X11Connection(x11rb::errors::ConnectionError),
}

impl AtomBlocksError {
    pub fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(self)
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

impl std::error::Error for AtomBlocksError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IOError(error) => Some(error),
            Self::ConfigRead { source, .. } => Some(source),
            Self::ConfigParse { source, .. } => Some(source),
            Self::X11Reply(error) => Some(error),
            Self::X11Connect(error) => Some(error),
            Self::X11Connection(error) => Some(error),
            Self::Config(_) | Self::Runtime(_) | Self::Interrupted(_) => None,
        }
    }
}

impl fmt::Display for AtomBlocksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IOError(error) => write!(f, "I/O error: {error}"),
            Self::Config(error) => write!(f, "configuration error: {error}"),
            Self::ConfigRead { path, source } => {
                write!(f, "failed to read config {}: {source}", path.display())
            }
            Self::ConfigParse { path, source } => {
                write!(f, "failed to parse config {}: {source}", path.display())
            }
            Self::Runtime(error) => f.write_str(error),
            Self::Interrupted(signal) => write!(f, "terminated by signal {signal}"),
            Self::X11Reply(error) => write!(f, "X11 reply error: {error}"),
            Self::X11Connect(error) => write!(f, "X11 connection error: {error}"),
            Self::X11Connection(error) => write!(f, "X11 connection error: {error}"),
        }
    }
}
