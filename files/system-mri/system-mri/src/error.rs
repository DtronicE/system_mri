//! Error type shared by every procfs reader.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("malformed {what}: {detail}")]
    Parse { what: &'static str, detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn parse(what: &'static str, detail: impl Into<String>) -> Self {
        Error::Parse {
            what,
            detail: detail.into(),
        }
    }

    /// `EACCES` / `EPERM`: the file exists but we are not allowed to read it.
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Error::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied)
    }

    /// `ENOENT` / `ESRCH`: the process (or file) is gone.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Io { source, .. }
            if source.kind() == io::ErrorKind::NotFound
                || source.raw_os_error() == Some(libc::ESRCH))
    }
}

pub(crate) fn read_file(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn read_bytes(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}
