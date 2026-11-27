//! Per-OS daemon endpoint naming: Unix socket files under
//! `<tmpdir>/beehive-<uid>/`; Windows named pipes in the `\\.\\pipe\\`
//! namespace (fixed daemon pipe, hashed worker pipes) - the TS product's
//! exact split.
use std::path::{Path, PathBuf};

use crate::paths::hash_key;

/// Default directory holding daemon socket files (Unix).
pub fn socket_dir() -> PathBuf {
    let uid = bh_core::platform::process::current_user_id();
    let tmp = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    socket_dir_for(&tmp, uid)
}

/// Reserve the longest generated endpoint before selecting a directory. Linux
/// can re-anchor deep paths through /proc; other Unix platforms need a short path.
fn socket_dir_for(tmp: &Path, uid: u32) -> PathBuf {
    let name = format!("beehive-{uid}");
    let dir = tmp.join(&name);
    #[cfg(not(target_os = "linux"))]
    if std::os::unix::net::SocketAddr::from_pathname(
        dir.join("worker-000000000000-000000000000.sock"),
    )
    .is_err()
    {
        return Path::new("/tmp").join(name);
    }
    dir
}


/// Default supervisor endpoint: `daemon.sock` in the socket dir (Unix) or
/// the fixed daemon pipe name (Windows).
#[must_use]
pub fn default_daemon_socket_path() -> PathBuf {
    socket_dir().join("daemon.sock")
}


/// Worker endpoint next to the supervisor's: hashed supervisor key plus the
/// worker id prefix (TS `workerSocketPath`).
#[must_use]
pub fn worker_socket_path(supervisor_socket_path: &Path, worker_id: &str) -> PathBuf {
    let key = hash_key(&supervisor_socket_path.to_string_lossy(), 12);
    socket_dir().join(format!(
        "worker-{key}-{}.sock",
        &worker_id[..12.min(worker_id.len())]
    ))
}


// Socket-filesystem identity is the shared platform contract
// `bh_types::platform::socket_identity`: the same helper serves stale-file
// cleanup here and direct-transport ticket validation in the clients.

pub use bh_types::daemon::SocketIdentity;
pub use bh_types::platform::socket_identity;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_socket_uses_the_same_real_uid_as_the_socket_dir() {
        let output = std::process::Command::new("id")
            .arg("-ru")
            .output()
            .unwrap();
        assert!(output.status.success());
        let uid = String::from_utf8(output.stdout).unwrap();
        let tmp = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        assert_eq!(
            socket_dir(),
            socket_dir_for(&tmp, uid.trim().parse().unwrap())
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn generated_endpoints_fit_even_when_tmpdir_is_deep() {
        use std::os::unix::net::SocketAddr;

        for length in [20, 65, 120] {
            let tmp = PathBuf::from(format!("/{}", "d".repeat(length - 1)));
            let dir = socket_dir_for(&tmp, 501);
            let worker = dir.join("worker-000000000000-000000000000.sock");
            assert!(SocketAddr::from_pathname(&worker).is_ok());
            if length >= 65 {
                assert_eq!(dir, Path::new("/tmp/beehive-501"));
            } else {
                assert_eq!(dir, tmp.join("beehive-501"));
            }
        }
    }

    #[test]
    fn short_tmpdir_keeps_the_original_socket_directory() {
        assert_eq!(
            socket_dir_for(Path::new("/tmp"), 501),
            Path::new("/tmp/beehive-501")
        );
    }

    #[test]
    fn worker_socket_names_are_deterministic() {
        let supervisor = Path::new("/tmp/beehive-1/daemon.sock");
        let a = worker_socket_path(supervisor, "0123456789abcdef");
        let b = worker_socket_path(supervisor, "fedcba9876543210");
        assert_ne!(a, b);
        // Only the first 12 id characters key the name.
        assert_eq!(a, worker_socket_path(supervisor, "0123456789abffff"));
        assert!(a.starts_with(socket_dir()));
    }
}
