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
        let canonical_target = path.canonicalize().map_err(|error| {
            if is_absent(error.kind()) {
                FilesystemProjectError::entry_not_found(path.to_path_buf(), error)
            } else {
                // Permission, loop, or name-resolution failures are not an
                // absent entry (#484).
                FilesystemProjectError::entry_unreadable(path.to_path_buf(), error)
            }
        })?;
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

/// Whether an I/O failure means the probed path cannot exist: the final
/// component is absent (`NotFound`), or an intermediate component is a
/// regular file (`NotADirectory` on Unix, a not-found error on Windows).
/// Both name an entry that does not exist rather than one that failed to
/// load (#497).
fn is_absent(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::NotFound | io::ErrorKind::NotADirectory)
}

fn default_entry(directory: &Path) -> Result<PathBuf, FilesystemProjectError> {
    let candidates = [directory.join("main.opy"), directory.join("src/main.opy")];
    for candidate in candidates {
        match fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(candidate),
            // Present but not a regular file, or absent — keep looking; only
            // a real I/O failure aborts classification (#484). A regular file
            // named `src` makes `src/main.opy` absent, so the probe reports
            // no default entry instead of an unreadable one (#497).
            Ok(_) => {}
            Err(error) if is_absent(error.kind()) => {}
            Err(error) => {
                return Err(FilesystemProjectError::entry_unreadable(candidate, error));
            }
        }
    }
    Err(FilesystemProjectError::default_entry_not_found(
        directory.to_path_buf(),
    ))
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
    /// An `EntryNotFound` carrying a non-absent I/O cause — possible
    /// through the public fields — reports the real category (#484). A path
    /// through a regular file counts as absent (`NotADirectory`, #497).
    pub fn is_entry_not_found(&self) -> bool {
        match self {
            Self::EntryNotFound { source, .. } => is_absent(source.kind()),
            Self::DefaultEntryNotFound { .. } => true,
            Self::EntryUnreadable { .. } => false,
        }
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
    use super::{FilesystemProject, FilesystemProjectError};
    use std::io;
    use std::path::{Path, PathBuf};

    #[test]
    fn directory_targets_use_the_owner_default_entry() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project-preprocessing");
        let project = FilesystemProject::load(&directory).expect("directory project loads");
        assert!(project.main_path().ends_with("main.opy"));
        assert_eq!(project.root(), directory.canonicalize().unwrap());
        assert!(project.source().contains("rule \"main\""));
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opy-project-io-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn absent_file_entry_is_entry_not_found() {
        let missing = scratch("missing").join("no-such.opy");
        let error = FilesystemProject::load(&missing).expect_err("absent entry fails");
        assert!(error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::EntryNotFound { .. }
        ));
    }

    /// A symlink cycle fails canonicalization with `Filesystem loop`, not
    /// `NotFound`: the failure must keep its real category instead of
    /// masquerading as an absent entry (#484).
    #[cfg(unix)]
    #[test]
    fn unresolvable_entry_is_not_classified_as_missing() {
        let dir = scratch("loop");
        let entry = dir.join("main.opy");
        std::os::unix::fs::symlink(Path::new("loop.opy"), dir.join("loop.opy")).unwrap();
        std::os::unix::fs::symlink(Path::new("main.opy"), &entry).unwrap();
        let error = FilesystemProject::load(&entry).expect_err("symlink cycle fails");
        assert!(!error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::EntryUnreadable { .. }
        ));
        let source = std::error::Error::source(&error).expect("io cause preserved");
        let io_error = source.downcast_ref::<io::Error>().expect("typed io::Error");
        assert_ne!(io_error.kind(), io::ErrorKind::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `main.opy` present but unresolvable: the default-entry search must
    /// report the I/O failure rather than `DefaultEntryNotFound` (#484).
    #[cfg(unix)]
    #[test]
    fn unresolvable_default_entry_is_not_classified_as_missing() {
        let dir = scratch("default-loop");
        std::os::unix::fs::symlink(Path::new("main.opy"), dir.join("main.opy")).unwrap();
        let error = FilesystemProject::load(&dir).expect_err("default entry cycle fails");
        assert!(!error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::EntryUnreadable { .. }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that resolves but cannot be read stays `EntryUnreadable`.
    /// Skipped under root, which bypasses permission checks.
    #[cfg(unix)]
    #[test]
    fn unreadable_entry_reports_unreadable() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = scratch("denied");
        let entry = dir.join("main.opy");
        std::fs::write(&entry, "rule \"x\": @Event global\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = FilesystemProject::load(&entry);
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let error = result.expect_err("unreadable entry fails");
        assert!(!error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::EntryUnreadable { .. }
        ));
    }

    /// A directory whose only `src` is a regular file has no default entry,
    /// the same as an empty directory (#497). Verified on Unix: `src` being
    /// a file makes the `src/main.opy` probe fail `ENOTDIR`. Windows reports
    /// a not-found error for the same probe, which the `is_absent`
    /// classification covers identically; CI on Windows currently runs only a
    /// subset of tests.
    #[test]
    fn file_named_src_is_default_entry_not_found() {
        let dir = scratch("file-src");
        std::fs::write(dir.join("src"), "not a directory\n").unwrap();
        let error = FilesystemProject::load(&dir).expect_err("no default entry");
        assert!(error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::DefaultEntryNotFound { .. }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An empty directory reports `DefaultEntryNotFound` — the baseline the
    /// file-named-`src` case must match (#497).
    #[test]
    fn empty_directory_is_default_entry_not_found() {
        let dir = scratch("empty");
        let error = FilesystemProject::load(&dir).expect_err("no default entry");
        assert!(error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::DefaultEntryNotFound { .. }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A path inside a regular file names an entry that does not exist, so it
    /// carries the not-found classification on every platform (#497). Verified
    /// on Unix, where canonicalization fails `ENOTDIR`; Windows reports a
    /// not-found error that lands in the same classification.
    #[test]
    fn entry_inside_a_file_is_entry_not_found() {
        let dir = scratch("notdir");
        let file = dir.join("main.opy");
        std::fs::write(&file, "rule \"x\": @Event global\n").unwrap();
        let entry = file.join("child.opy");
        let error = FilesystemProject::load(&entry).expect_err("ENOTDIR fails");
        assert!(error.is_entry_not_found());
        assert!(matches!(
            error,
            FilesystemProjectError::EntryNotFound { .. }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
