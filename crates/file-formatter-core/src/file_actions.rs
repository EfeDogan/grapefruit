use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOperation {
    Copy,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferJob {
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    pub operation: TransferOperation,
    pub jobs: Vec<TransferJob>,
}

#[derive(Debug)]
pub enum FileActionError {
    NoInput,
    CurrentDirectoryUnavailable(io::Error),
    SourceUnavailable(PathBuf, io::Error),
    DestinationUnavailable(PathBuf, io::Error),
    InvalidSourceName(PathBuf),
    SameLocation(PathBuf),
    DestinationInsideSource {
        source: PathBuf,
        destination: PathBuf,
    },
    UnsupportedSymlink(PathBuf),
    DestinationAlreadyExists(PathBuf),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for FileActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInput => formatter.write_str("no input files or directories were provided"),
            Self::CurrentDirectoryUnavailable(error) => {
                write!(
                    formatter,
                    "could not resolve the current directory: {error}"
                )
            }
            Self::SourceUnavailable(path, error) => {
                write!(
                    formatter,
                    "source is unavailable ({}): {error}",
                    path.display()
                )
            }
            Self::DestinationUnavailable(path, error) => write!(
                formatter,
                "destination directory is unavailable ({}): {error}",
                path.display()
            ),
            Self::InvalidSourceName(path) => {
                write!(
                    formatter,
                    "source has no usable file name: {}",
                    path.display()
                )
            }
            Self::SameLocation(path) => write!(
                formatter,
                "refusing to move an item into its current directory: {}",
                path.display()
            ),
            Self::DestinationInsideSource {
                source,
                destination,
            } => write!(
                formatter,
                "refusing to place {} inside itself ({})",
                source.display(),
                destination.display()
            ),
            Self::UnsupportedSymlink(path) => write!(
                formatter,
                "symbolic links are not supported by file actions yet: {}",
                path.display()
            ),
            Self::DestinationAlreadyExists(path) => write!(
                formatter,
                "refusing to overwrite an existing destination: {}",
                path.display()
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} {}: {source}", path.display()),
        }
    }
}

impl Error for FileActionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CurrentDirectoryUnavailable(error)
            | Self::SourceUnavailable(_, error)
            | Self::DestinationUnavailable(_, error)
            | Self::Io { source: error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Builds a transfer plan and reserves conflict-free destination names without
/// changing the filesystem. Sources and the destination directory are
/// canonicalized so self-referential directory operations can be rejected.
pub fn plan_transfer(
    sources: &[PathBuf],
    destination_directory: &Path,
    operation: TransferOperation,
) -> Result<TransferPlan, FileActionError> {
    if sources.is_empty() {
        return Err(FileActionError::NoInput);
    }

    let current_directory =
        std::env::current_dir().map_err(FileActionError::CurrentDirectoryUnavailable)?;
    let requested_destination = absolute_path(destination_directory, &current_directory);
    // `std::fs::canonicalize` returns verbatim `\\?\` paths on Windows.
    // `dunce` keeps the canonical comparison semantics while returning the
    // conventional drive-letter form whenever it can be represented safely.
    let destination = dunce::canonicalize(&requested_destination).map_err(|error| {
        FileActionError::DestinationUnavailable(requested_destination.clone(), error)
    })?;
    let destination_metadata = fs::metadata(&destination)
        .map_err(|error| FileActionError::DestinationUnavailable(destination.clone(), error))?;
    if !destination_metadata.is_dir() {
        return Err(FileActionError::DestinationUnavailable(
            destination,
            io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
        ));
    }

    let mut reserved_destinations = Vec::with_capacity(sources.len());
    let mut jobs = Vec::with_capacity(sources.len());

    for source in sources {
        let requested_source = absolute_path(source, &current_directory);
        let source_metadata = fs::symlink_metadata(&requested_source)
            .map_err(|error| FileActionError::SourceUnavailable(requested_source.clone(), error))?;
        if source_metadata.file_type().is_symlink() {
            return Err(FileActionError::UnsupportedSymlink(requested_source));
        }
        if !source_metadata.is_file() && !source_metadata.is_dir() {
            return Err(FileActionError::SourceUnavailable(
                requested_source,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file or directory",
                ),
            ));
        }

        let source = dunce::canonicalize(&requested_source)
            .map_err(|error| FileActionError::SourceUnavailable(requested_source, error))?;
        let file_name = source
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| FileActionError::InvalidSourceName(source.clone()))?;

        if operation == TransferOperation::Move
            && source.parent().is_some_and(|parent| parent == destination)
        {
            return Err(FileActionError::SameLocation(source));
        }
        if source_metadata.is_dir() && destination.starts_with(&source) {
            return Err(FileActionError::DestinationInsideSource {
                source,
                destination,
            });
        }

        let output = unique_destination_path(
            &destination,
            file_name,
            source_metadata.is_dir(),
            &reserved_destinations,
        );
        reserved_destinations.push(output.clone());
        jobs.push(TransferJob {
            source,
            destination: output,
        });
    }

    Ok(TransferPlan { operation, jobs })
}

/// Executes a previously validated transfer plan. New files are created with
/// exclusive creation so an item appearing after planning is never overwritten.
pub fn execute_transfer(plan: &TransferPlan) -> Result<Vec<PathBuf>, FileActionError> {
    let mut completed = Vec::with_capacity(plan.jobs.len());

    for job in &plan.jobs {
        if job.destination.exists() {
            return Err(FileActionError::DestinationAlreadyExists(
                job.destination.clone(),
            ));
        }

        match plan.operation {
            TransferOperation::Copy => copy_entry(&job.source, &job.destination)?,
            TransferOperation::Move => move_entry(&job.source, &job.destination)?,
        }
        completed.push(job.destination.clone());
    }

    Ok(completed)
}

fn move_entry(source: &Path, destination: &Path) -> Result<(), FileActionError> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_entry(source, destination)?;
            let metadata = fs::symlink_metadata(source)
                .map_err(|error| io_error("inspect source", source, error))?;
            let remove_result = if metadata.is_dir() {
                fs::remove_dir_all(source)
            } else {
                fs::remove_file(source)
            };
            // If cleanup fails, keep the completed destination intact. Removing
            // it here could lose data when the source directory was only
            // partially deleted.
            remove_result
                .map_err(|error| io_error("remove source after cross-device move", source, error))
        }
        Err(error) => Err(io_error("move", source, error)),
    }
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), FileActionError> {
    let result = copy_entry_inner(source, destination);
    if result.is_err() {
        let _ = remove_entry(destination);
    }
    result
}

fn copy_entry_inner(source: &Path, destination: &Path) -> Result<(), FileActionError> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| io_error("inspect source", source, error))?;
    if metadata.file_type().is_symlink() {
        return Err(FileActionError::UnsupportedSymlink(source.to_path_buf()));
    }

    if metadata.is_file() {
        return copy_file(source, destination, &metadata);
    }
    if !metadata.is_dir() {
        return Err(FileActionError::SourceUnavailable(
            source.to_path_buf(),
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file or directory",
            ),
        ));
    }

    fs::create_dir(destination).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            FileActionError::DestinationAlreadyExists(destination.to_path_buf())
        } else {
            io_error("create destination directory", destination, error)
        }
    })?;

    let entries =
        fs::read_dir(source).map_err(|error| io_error("read source directory", source, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read source directory", source, error))?;
        copy_entry_inner(&entry.path(), &destination.join(entry.file_name()))?;
    }

    fs::set_permissions(destination, metadata.permissions())
        .map_err(|error| io_error("preserve directory permissions", destination, error))
}

fn copy_file(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
) -> Result<(), FileActionError> {
    let mut input =
        fs::File::open(source).map_err(|error| io_error("open source", source, error))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                FileActionError::DestinationAlreadyExists(destination.to_path_buf())
            } else {
                io_error("create destination", destination, error)
            }
        })?;
    io::copy(&mut input, &mut output)
        .map_err(|error| io_error("copy file contents", destination, error))?;
    output
        .sync_all()
        .map_err(|error| io_error("flush destination", destination, error))?;
    fs::set_permissions(destination, metadata.permissions())
        .map_err(|error| io_error("preserve file permissions", destination, error))
}

fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn unique_destination_path(
    directory: &Path,
    file_name: &OsStr,
    is_directory: bool,
    reserved: &[PathBuf],
) -> PathBuf {
    let original = Path::new(file_name);
    let stem = if is_directory {
        file_name
    } else {
        original.file_stem().unwrap_or(file_name)
    };
    let extension = if is_directory {
        None
    } else {
        original.extension()
    };

    for index in 0_u32.. {
        let mut name = OsString::from(stem);
        if index > 0 {
            name.push(format!(" ({index})"));
        }
        if let Some(extension) = extension {
            name.push(".");
            name.push(extension);
        }
        let candidate = directory.join(name);
        if !candidate.exists() && !reserved.contains(&candidate) {
            return candidate;
        }
    }

    unreachable!("the destination suffix space is not practically exhaustible")
}

fn absolute_path(path: &Path, current_directory: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_directory.join(path)
    }
}

fn io_error(action: &'static str, path: &Path, source: io::Error) -> FileActionError {
    FileActionError::Io {
        action,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "file-formatter-actions-{}-{name}-{timestamp}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn plans_conflict_free_names_for_files_and_directories() {
        let root = TestDirectory::new("conflicts");
        let source_root = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(source_root.join("album")).expect("create source folder");
        fs::create_dir_all(destination.join("album")).expect("create existing folder");
        fs::write(source_root.join("notes.txt"), b"new").expect("create source file");
        fs::write(destination.join("notes.txt"), b"old").expect("create existing file");

        let plan = plan_transfer(
            &[source_root.join("notes.txt"), source_root.join("album")],
            &destination,
            TransferOperation::Copy,
        )
        .expect("plan copy");

        let canonical_destination =
            dunce::canonicalize(&destination).expect("canonicalize destination");
        assert_eq!(
            plan.jobs[0].destination,
            canonical_destination.join("notes (1).txt")
        );
        assert_eq!(
            plan.jobs[1].destination,
            canonical_destination.join("album (1)")
        );
    }

    #[test]
    fn copies_files_and_directories_without_changing_sources() {
        let root = TestDirectory::new("copy");
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(source.join("nested")).expect("create source folder");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(source.join("nested").join("note.txt"), b"hello").expect("create fixture");

        let plan = plan_transfer(
            std::slice::from_ref(&source),
            &destination,
            TransferOperation::Copy,
        )
        .expect("plan copy");
        let outputs = execute_transfer(&plan).expect("execute copy");

        assert!(source.join("nested").join("note.txt").is_file());
        assert_eq!(
            fs::read(outputs[0].join("nested").join("note.txt")).expect("read copied file"),
            b"hello"
        );
    }

    #[test]
    fn moves_a_file_and_removes_the_source() {
        let root = TestDirectory::new("move");
        let source_directory = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source_directory).expect("create source folder");
        fs::create_dir_all(&destination).expect("create destination");
        let source = source_directory.join("note.txt");
        fs::write(&source, b"hello").expect("create fixture");

        let plan = plan_transfer(
            std::slice::from_ref(&source),
            &destination,
            TransferOperation::Move,
        )
        .expect("plan move");
        let outputs = execute_transfer(&plan).expect("execute move");

        assert!(!source.exists());
        assert_eq!(fs::read(&outputs[0]).expect("read moved file"), b"hello");
    }

    #[test]
    fn rejects_a_move_to_the_current_directory() {
        let root = TestDirectory::new("same-directory");
        let source = root.path().join("note.txt");
        fs::write(&source, b"hello").expect("create fixture");

        let error = plan_transfer(
            std::slice::from_ref(&source),
            root.path(),
            TransferOperation::Move,
        )
        .expect_err("reject same directory");

        let canonical_source = dunce::canonicalize(&source).expect("canonicalize source");
        assert!(matches!(error, FileActionError::SameLocation(path) if path == canonical_source));
    }

    #[test]
    fn rejects_a_directory_destination_inside_itself() {
        let root = TestDirectory::new("recursive");
        let source = root.path().join("source");
        let child = source.join("child");
        fs::create_dir_all(&child).expect("create nested folder");

        let error = plan_transfer(
            std::slice::from_ref(&source),
            &child,
            TransferOperation::Copy,
        )
        .expect_err("reject recursive copy");

        assert!(matches!(
            error,
            FileActionError::DestinationInsideSource { .. }
        ));
    }

    #[test]
    fn rejects_an_output_created_after_planning() {
        let root = TestDirectory::new("late-conflict");
        let source_directory = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source_directory).expect("create source folder");
        fs::create_dir_all(&destination).expect("create destination");
        let source = source_directory.join("note.txt");
        fs::write(&source, b"new").expect("create fixture");
        let plan = plan_transfer(
            std::slice::from_ref(&source),
            &destination,
            TransferOperation::Copy,
        )
        .expect("plan copy");
        fs::write(&plan.jobs[0].destination, b"existing").expect("create conflict");

        let error = execute_transfer(&plan).expect_err("reject conflict");

        assert!(matches!(
            error,
            FileActionError::DestinationAlreadyExists(_)
        ));
        assert_eq!(
            fs::read(&plan.jobs[0].destination).expect("read existing file"),
            b"existing"
        );
    }
}
