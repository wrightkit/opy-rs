use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A filesystem-backed OPY project entry and the root used to resolve it.
#[derive(Debug)]
pub struct FilesystemProject {
    source: String,
    main_path: PathBuf,
    root: PathBuf,
}

impl FilesystemProject {
    /// Load an OPY project from its selected filesystem entry.
    pub fn load(path: &Path) -> Result<Self, FilesystemProjectError> {
        let canonical_path = path
            .canonicalize()
            .map_err(|error| FilesystemProjectError::entry_not_found(path.to_path_buf(), error))?;
        let source = fs::read_to_string(&canonical_path).map_err(|error| {
            FilesystemProjectError::entry_unreadable(canonical_path.clone(), error)
        })?;
        let root = canonical_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            source,
            main_path: path.to_path_buf(),
            root,
        })
    }

    /// The source text read from the selected entry.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The selected entry path used as the source identity in diagnostics.
    pub fn main_path(&self) -> &Path {
        &self.main_path
    }

    /// The canonical parent directory used for project-relative resolution.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Failure while loading a filesystem-backed OPY project entry.
#[derive(Debug)]
pub enum FilesystemProjectError {
    EntryNotFound { path: PathBuf, source: io::Error },
    EntryUnreadable { path: PathBuf, source: io::Error },
}

impl FilesystemProjectError {
    fn entry_not_found(path: PathBuf, source: io::Error) -> Self {
        Self::EntryNotFound { path, source }
    }

    fn entry_unreadable(path: PathBuf, source: io::Error) -> Self {
        Self::EntryUnreadable { path, source }
    }

    /// Whether the entry could not be resolved to a canonical filesystem path.
    pub fn is_entry_not_found(&self) -> bool {
        matches!(self, Self::EntryNotFound { .. })
    }
}

impl fmt::Display for FilesystemProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryNotFound { source, .. } => write!(formatter, "cannot find entry: {source}"),
            Self::EntryUnreadable { source, .. } => {
                write!(formatter, "cannot read entry: {source}")
            }
        }
    }
}

impl std::error::Error for FilesystemProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EntryNotFound { source, .. } | Self::EntryUnreadable { source, .. } => {
                Some(source)
            }
        }
    }
}
