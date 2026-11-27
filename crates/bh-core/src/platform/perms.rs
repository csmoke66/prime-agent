//! File permission policy. Unix: owner-only mode bits (0o600 files, 0o700
//! dirs). Windows: NTFS ACLs govern access - new files inherit ACLs from
//! their parent directory, so the restriction helpers are no-ops there.

use std::fs::OpenOptions;
use std::path::Path;

/// Owner-only file mode (Unix).
pub const PRIVATE_FILE_MODE: u32 = 0o600;
/// Owner-only directory mode (Unix).
pub const PRIVATE_DIR_MODE: u32 = 0o700;

/// Make a file owner-readable/writable only (`chmod 0o600`). Best-effort:
/// callers decide whether a failure is fatal.
///
/// # Errors
///
/// Returns the underlying I/O error when the permission change fails.
pub fn restrict_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
}


/// Make a directory owner-accessible only (`chmod 0o700`).
///
/// # Errors
///
/// Returns the underlying I/O error when the permission change fails.
pub fn restrict_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_DIR_MODE))
}


/// Set the private mode on files created through these options.
pub fn set_private_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(PRIVATE_FILE_MODE);
}


/// The file's mode bits (`mode & 0o777`); None where mode bits do not exist.
#[must_use]
pub fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}


/// True when the path is an executable file (any execute bit on Unix).
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}


/// Set the private mode on an already-open file (`fchmod`): exact bits despite
/// the umask; tightens a pre-existing loose file.
///
/// # Errors
///
/// The underlying I/O error when the permission bits cannot be set.
pub fn restrict_open_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
}


/// Create directories recursively with the private dir mode on platforms with
/// mode bits; existing directories are left untouched (mkdir semantics).
///
/// # Errors
///
/// Returns the underlying I/O error when a directory cannot be created.
pub fn create_dir_all_private(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(PRIVATE_DIR_MODE)
        .recursive(true)
        .create(path)
}


