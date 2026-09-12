use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A filesystem-backed OPY project target and the root used to resolve it.
#[derive(Debug)]
pub struct FilesystemProject {
    source: String,
    main_path: PathBuf,
    root: PathBuf,
}

impl FilesystemProject {
    /// Load an OPY project from a file entry or a project directory.
    pub fn load(path: &Path) -> Result<Self, FilesystemProjectError> {
        let canonical_target = path
            .canonicalize()
            .map_err(|error| FilesystemProjectError::entry_not_found(path.to_path_buf(), error))?;
        let selected_path = if canonical_target.is_dir() {
            default_entry(&canonical_target)?
        } else {
            canonical_target.clone()
        };
        let source = fs::read_to_string(&selected_path).map_err(|error| {
            FilesystemProjectError::entry_unreadable(selected_path.clone(), error)
        })?;
        let root = selected_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            source,
            main_path: if canonical_target.is_dir() {
                selected_path
            } else {
                path.to_path_buf()
            },
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

fn default_entry(directory: &Path) -> Result<PathBuf, FilesystemProjectError> {
    let candidates = [directory.join("main.opy"), directory.join("src/main.opy")];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| FilesystemProjectError::default_entry_not_found(directory.to_path_buf()))
}

/// Failure while loading a filesystem-backed OPY project entry.
#[derive(Debug)]
pub enum FilesystemProjectError {
    EntryNotFound { path: PathBuf, source: io::Error },
    EntryUnreadable { path: PathBuf, source: io::Error },
    DefaultEntryNotFound { path: PathBuf },
}

impl FilesystemProjectError {
    fn entry_not_found(path: PathBuf, source: io::Error) -> Self {
        Self::EntryNotFound { path, source }
    }

    fn entry_unreadable(path: PathBuf, source: io::Error) -> Self {
        Self::EntryUnreadable { path, source }
    }

    fn default_entry_not_found(path: PathBuf) -> Self {
        Self::DefaultEntryNotFound { path }
    }

    /// Whether the entry could not be resolved to a canonical filesystem path.
    pub fn is_entry_not_found(&self) -> bool {
        matches!(
            self,
            Self::EntryNotFound { .. } | Self::DefaultEntryNotFound { .. }
        )
    }
}

impl fmt::Display for FilesystemProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryNotFound { source, .. } => write!(formatter, "cannot find entry: {source}"),
            Self::EntryUnreadable { source, .. } => {
                write!(formatter, "cannot read entry: {source}")
            }
            Self::DefaultEntryNotFound { path } => {
                write!(
                    formatter,
                    "cannot find a default OPY entry in '{}'; expected main.opy or src/main.opy",
                    path.display()
                )
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
            Self::DefaultEntryNotFound { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FilesystemProject;

    #[test]
    fn directory_targets_use_the_owner_default_entry() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project-preprocessing");
        let project = FilesystemProject::load(&directory).expect("directory project loads");
        assert!(project.main_path().ends_with("main.opy"));
        assert_eq!(project.root(), directory.canonicalize().unwrap());
        assert!(project.source().contains("rule \"main\""));
    }
}
