use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShelfError {
    #[error("{0}")]
    Validation(String),

    #[error("source not found: {0}")]
    SourceNotFound(String),

    #[error("no default source configured")]
    MissingDefaultSource,

    #[error("mount not found for local path: {0}")]
    MountNotFound(String),

    #[error("command failed: {program} {args:?} exited with {status}: {stderr}")]
    CommandFailed {
        program: String,
        args: Vec<String>,
        status: i32,
        stderr: String,
    },

    #[error("failed to run command {program}: {source}")]
    CommandIo {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("operation requires root privileges")]
    RequiresRoot,

    #[error("config file not found: {0}")]
    ConfigNotFound(PathBuf),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("toml parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("toml encode error: {0}")]
    TomlEncode(#[from] toml::ser::Error),

    #[error("invalid run mount spec, expected <local-path>:<remote-path>: {0}")]
    InvalidRunMount(String),
}

pub type Result<T> = std::result::Result<T, ShelfError>;

pub trait IoContext<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> Result<T> {
        let path = path.into();
        self.map_err(|source| ShelfError::Io { path, source })
    }
}
