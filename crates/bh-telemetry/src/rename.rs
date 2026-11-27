//! Rename-onto-destination for persisted files.

use std::fs;
use std::io;
use std::path::Path;

/// Rename `from` onto `to`, replacing an existing destination.
///
/// # Errors
///
/// Returns the underlying `std::fs::rename` error.
pub fn rename_onto(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_onto_replaces_destination() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.json");
        let to = dir.path().join("to.json");
        std::fs::write(&from, b"next").unwrap();
        std::fs::write(&to, b"previous").unwrap();
        rename_onto(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "next");
        assert!(!from.exists());
    }

    #[test]
    fn rename_onto_missing_source_surfaces_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("missing");
        let to = dir.path().join("to.json");
        let error = rename_onto(&from, &to).expect_err("rename of a missing source must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
