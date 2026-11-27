//! Transport contract for daemon sockets and streams: `AF_UNIX` sockets today, named pipes on
//! Windows later. Callers bind/connect through these traits and never name a concrete socket type.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use anyhow::Result;
// `Context` is used only by the linux `O_PATH` re-anchoring and the
// Windows pipe-name error; a bare import is unused elsewhere.
#[cfg(target_os = "linux")]
use anyhow::Context;

/// A full-duplex stream between a client and a daemon endpoint; `split`
/// consumes it into owned halves, and callers hold only the erased halves.
pub trait TransportStream: Send + Sync {
    fn split(self: Box<Self>) -> (Box<dyn AsyncReadHalf>, Box<dyn AsyncWriteHalf>);
}

/// Owned read half of a [`TransportStream`]; blanket-implemented.
pub trait AsyncReadHalf: tokio::io::AsyncRead + Unpin + Send {}
impl<T> AsyncReadHalf for T where T: tokio::io::AsyncRead + Unpin + Send {}

/// Owned write half of a [`TransportStream`]; blanket-implemented.
pub trait AsyncWriteHalf: tokio::io::AsyncWrite + Unpin + Send {}
impl<T> AsyncWriteHalf for T where T: tokio::io::AsyncWrite + Unpin + Send {}

impl TransportStream for tokio::net::UnixStream {
    fn split(self: Box<Self>) -> (Box<dyn AsyncReadHalf>, Box<dyn AsyncWriteHalf>) {
        let (reader, writer) = tokio::net::UnixStream::into_split(*self);
        (Box::new(reader), Box::new(writer))
    }
}

/// Future returned by [`TransportListener::accept`].
pub type AcceptFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<Box<dyn TransportStream>>> + Send + 'a>>;

/// A bound transport endpoint that hands out connected streams.
pub trait TransportListener: Send + Sync {
    /// Future-boxed so the trait stays dyn-compatible (RPITIT methods are not).
    fn accept(&self) -> AcceptFuture<'_>;
}

impl TransportListener for tokio::net::UnixListener {
    fn accept(&self) -> AcceptFuture<'_> {
        Box::pin(async move {
            let (stream, _address) = self.accept().await?;
            Ok(Box::new(stream) as Box<dyn TransportStream>)
        })
    }
}

/// Native `AF_UNIX` path capacity, excluding the terminating NUL.
const MAX_SUN_PATH: usize = {
    // SAFETY: sockaddr_un contains only integer fields and arrays of integers.
    let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_path.len() - 1
};

/// A kernel-valid `AF_UNIX` address for `bind`/`connect`. Paths within the limit pass through; a
/// longer path is re-anchored through an `O_PATH` descriptor on its parent
/// (`/proc/self/fd/<fd>/<name>`) so the socket file still lands at the original deep location (like
/// TS, surviving deep `TMPDIR` paths). Linux only.
pub struct UnixSocketAddress {
    address: std::path::PathBuf,
    /// Holds the directory descriptor open; the re-anchored
    /// `/proc/self/fd` path is valid only while this lives.
    _dir: Option<std::fs::File>,
}

impl UnixSocketAddress {
    /// The effective address to hand to `bind`/`connect`.
    fn effective(&self) -> &Path {
        &self.address
    }

    /// Resolve `path` into a kernel-valid `AF_UNIX` address, or fail with the
    /// original path in the message.
    fn new(path: &Path) -> Result<Self> {
        if path.as_os_str().len() <= MAX_SUN_PATH {
            return Ok(Self {
                address: path.to_path_buf(),
                _dir: None,
            });
        }
        #[cfg(target_os = "linux")]
        {
            let (address, dir) = Self::through_dir_fd(path)?;
            Ok(Self {
                address,
                _dir: Some(dir),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            anyhow::bail!(
                "AF_UNIX socket path exceeds the {MAX_SUN_PATH}-byte limit: {}",
                path.display()
            )
        }
    }

    /// Re-anchor a too-long path through `/proc/self/fd/<dir fd>/<file name>`.
    #[cfg(target_os = "linux")]
    fn through_dir_fd(path: &Path) -> Result<(std::path::PathBuf, std::fs::File)> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        // `O_PATH`: an fd that references the directory without read/write
        // access; only `/proc/self/fd` traversal uses it.
        const O_PATH: i32 = 0o200_000;
        let name = path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("socket path has no file name: {}", path.display()))?;
        let parent = path.parent().ok_or_else(|| {
            anyhow::anyhow!("socket path has no parent directory: {}", path.display())
        })?;
        let dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_PATH)
            .open(parent)
            .with_context(|| format!("open socket directory {}", parent.display()))?;
        let address = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            dir.as_raw_fd(),
            name.to_string_lossy()
        ));
        if address.as_os_str().len() > MAX_SUN_PATH {
            anyhow::bail!(
                "AF_UNIX socket path exceeds the {MAX_SUN_PATH}-byte limit: {}",
                path.display()
            );
        }
        Ok((address, dir))
    }
}

/// Bind a listening endpoint at `path` (a socket file on Unix).
///
/// # Errors
///
/// Returns an error if `path` cannot become a kernel-valid socket address or the listener fails to
/// bind.
pub async fn bind_transport(path: &Path) -> Result<Box<dyn TransportListener>> {
    let address = UnixSocketAddress::new(path)?;
    let listener = tokio::net::UnixListener::bind(address.effective())?;
    Ok(Box::new(listener))
}

/// Connect to the endpoint at `path` asynchronously.
///
/// # Errors
///
/// Returns an error if `path` cannot become a kernel-valid socket address or the connection fails.
pub async fn connect_transport(path: &Path) -> Result<Box<dyn TransportStream>> {
    let address = UnixSocketAddress::new(path)?;
    let stream = tokio::net::UnixStream::connect(address.effective()).await?;
    ensure_same_user_peer(std::os::fd::AsFd::as_fd(&stream), path)?;
    Ok(Box::new(stream))
}

/// Refuse a socket served by another user's process: whoever controls the
/// socket's directory could otherwise answer as the daemon.
fn ensure_same_user_peer(fd: std::os::fd::BorrowedFd<'_>, path: &Path) -> std::io::Result<()> {
    let peer_uid = peer_uid(fd)?;
    // SAFETY: geteuid only reads this process's effective uid.
    let uid = unsafe { libc::geteuid() };
    if peer_uid != uid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "Socket {} is served by uid {peer_uid}, not the current user (uid {uid})",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn peer_uid(fd: std::os::fd::BorrowedFd<'_>) -> std::io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    // `ucred` is 12 bytes.
    #[allow(clippy::cast_possible_truncation)]
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED writes at most `len` bytes into `cred`.
    let status = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(cred.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(fd: std::os::fd::BorrowedFd<'_>) -> std::io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: getpeereid writes the peer's ids into the two locals.
    if unsafe { libc::getpeereid(fd.as_raw_fd(), &raw mut uid, &raw mut gid) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(uid)
}

/// Definitive nonblocking Unix listener refusal, used by the exit cleanup.
/// A saturated backlog can yield `EAGAIN` on Linux rather than a completed
/// connect. Only `ConnectionRefused` permits unlink; all unknown outcomes fail
/// closed. Reuse the transport's long-path re-anchor so deep socket paths
/// retain their stale-cleanup behavior.
///
/// The refused verdict is definitive on Linux only: on macOS and the BSDs a
/// saturated listen queue ALSO refuses the connect, so a live successor that
/// filled its backlog is indistinguishable from a dead path by this probe.
/// There the function never claims closure (the exit cleanup preserves the
/// path; the next bind's stale-socket prepare cleans a file nothing serves).
#[must_use]
pub fn unix_listener_definitely_closed(path: &Path) -> bool {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        false
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(effective) = UnixSocketAddress::new(path) else {
            return false;
        };
        let Ok(address) = socket2::SockAddr::unix(effective.effective()) else {
            return false;
        };
        let Ok(socket) = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
        else {
            return false;
        };
        if socket.set_nonblocking(true).is_err() {
            return false;
        }
        matches!(
            socket.connect(&address),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused
        )
    }
}









/// A blocking full-duplex stream, for the CLI's one-shot command client.
pub trait BlockingTransportStream:
    std::fmt::Debug + std::io::Read + std::io::Write + Send + Sync
{
    /// Duplicate the underlying handle so reads and writes can proceed on separate owned halves.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle cannot be duplicated.
    fn try_clone_box(&self) -> std::io::Result<Box<dyn BlockingTransportStream>>;
    /// Deadline a pending read (poll granularity for deadline-driven waits).
    ///
    /// # Errors
    ///
    /// Returns an error if setting the read deadline on the underlying stream fails.
    fn set_read_timeout(&self, timeout: std::time::Duration) -> std::io::Result<()>;
}

impl BlockingTransportStream for std::os::unix::net::UnixStream {
    fn try_clone_box(&self) -> std::io::Result<Box<dyn BlockingTransportStream>> {
        Ok(Box::new(self.try_clone()?))
    }

    fn set_read_timeout(&self, timeout: std::time::Duration) -> std::io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, Some(timeout))
    }
}

/// Connect to the endpoint at `path`, blocking until connected.
///
/// # Errors
///
/// Returns an error if `path` cannot become a kernel-valid socket address or the blocking
/// connection fails.
pub fn connect_blocking(path: &Path) -> std::io::Result<Box<dyn BlockingTransportStream>> {
    let address = UnixSocketAddress::new(path).map_err(std::io::Error::other)?;
    let stream = std::os::unix::net::UnixStream::connect(address.effective())?;
    ensure_same_user_peer(std::os::fd::AsFd::as_fd(&stream), path)?;
    Ok(Box::new(stream))
}


#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::FileTypeExt;

    /// A directory whose full path length is exactly `target` bytes; falls
    /// back to `/tmp` when the ambient `TMPDIR` is already too deep.
    fn dir_of_exact_len(tag: &str, target: usize) -> std::path::PathBuf {
        let tag = format!("pa-transport-sun-path-{tag}");
        let base = std::env::temp_dir().join(&tag);
        let base = if base.as_os_str().len() + 21 <= target {
            base
        } else {
            std::path::Path::new("/tmp").join(&tag)
        };
        let mut dir = base;
        // Keep at least one byte of room for a file name after the separator.
        while dir.as_os_str().len() + 22 <= target {
            dir = dir.join("d".repeat(20));
        }
        // Pad one final component: appending adds a separator plus the name.
        let pad = target
            .checked_sub(dir.as_os_str().len() + 1)
            .expect("base must leave room for a file name");
        dir = dir.join("d".repeat(pad));
        assert_eq!(dir.as_os_str().len(), target);
        std::fs::create_dir_all(&dir).expect("create deep dir");
        dir
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn over_limit_paths_bind_connect_and_land_in_place() {
        use tokio::io::AsyncReadExt;
        let dir = dir_of_exact_len("roundtrip", 120);
        let socket = dir.join("worker-test.sock");
        let _ = std::fs::remove_file(&socket);
        assert!(socket.as_os_str().len() > MAX_SUN_PATH);

        bind_transport(&socket)
            .await
            .expect("bind through the limit");
        assert!(std::fs::symlink_metadata(&socket)
            .expect("socket file at the original deep path")
            .file_type()
            .is_socket());
        // A second bind on the live socket must fail (address in use), not
        // silently re-anchor somewhere else.
        assert!(bind_transport(&socket).await.is_err());
        std::fs::remove_file(&socket).expect("cleanup for rebind");

        let listener =
            tokio::net::UnixListener::bind(UnixSocketAddress::new(&socket).unwrap().effective())
                .expect("rebind address");
        let connect_path = socket.clone();
        let connect = tokio::spawn(async move {
            connect_transport(&connect_path)
                .await
                .expect("connect through the limit")
        });
        let (server, _) = listener.accept().await.expect("accept");
        let client = connect.await.expect("client task");
        // Round-trip one write to prove the pair is the same socket.
        let (mut reader, _writer) = client.split();
        server.writable().await.expect("server writable");
        server.try_write(b"ping").expect("server write");
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).await.expect("client read");
        assert_eq!(&buf, b"ping");
        let _ = std::fs::remove_file(&socket);
    }

    #[tokio::test]
    async fn paths_at_the_limit_bind_directly() {
        let dir = dir_of_exact_len("boundary", 96);
        let name = "x".repeat(MAX_SUN_PATH - dir.as_os_str().len() - 1);
        let socket = dir.join(name);
        assert_eq!(socket.as_os_str().len(), MAX_SUN_PATH);
        bind_transport(&socket)
            .await
            .expect("bind at exactly the limit");
        let _ = std::fs::remove_file(&socket);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn paths_one_byte_over_the_native_limit_are_rejected_with_context() {
        let path = std::path::PathBuf::from(format!("/{}", "x".repeat(MAX_SUN_PATH)));
        let error = UnixSocketAddress::new(&path)
            .err()
            .expect("over native limit");
        assert_eq!(
            error.to_string(),
            format!(
                "AF_UNIX socket path exceeds the {MAX_SUN_PATH}-byte limit: {}",
                path.display()
            )
        );
    }

    #[tokio::test]
    async fn over_limit_paths_without_a_short_name_error_clearly() {
        let dir = dir_of_exact_len("toolong", 120);
        let socket = dir.join("n".repeat(120));
        assert!(socket.as_os_str().len() > MAX_SUN_PATH);
        let error = bind_transport(&socket)
            .await
            .err()
            .expect("no short address exists");
        assert!(error.to_string().contains("exceeds"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

