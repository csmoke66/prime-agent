use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::{open, openat, AtFlags, OFlag};
use nix::sys::stat::{fstatat, mkdirat, FileStat, Mode, SFlag};
use nix::unistd::{linkat, unlinkat, UnlinkatFlags};

const DIR_FLAGS: OFlag = OFlag::O_DIRECTORY
    .union(OFlag::O_CLOEXEC)
    .union(OFlag::O_NOFOLLOW);

pub(super) struct Directory {
    file: File,
    operator: bool,
}

fn open_file(fd: nix::Result<libc::c_int>) -> io::Result<File> {
    let fd = fd?;
    // SAFETY: a successful open returns a fresh descriptor owned by us.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn unsafe_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn require_protected(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(unsafe_path(
            "operator workflow paths must be root-owned and not group/world-writable",
        ));
    }
    #[cfg(target_os = "macos")]
    require_no_mutating_acl(file)?;
    Ok(())
}

// macOS ACLs can grant write access without changing the POSIX mode bits.
#[cfg(target_os = "macos")]
fn require_no_mutating_acl(file: &File) -> io::Result<()> {
    use std::ffi::c_void;
    use std::ptr;

    extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, acl_type: libc::c_int) -> *mut c_void;
        fn acl_get_entry(
            acl: *mut c_void,
            entry_id: libc::c_int,
            entry: *mut *mut c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut libc::c_uint) -> libc::c_int;
        fn acl_get_permset_mask_np(entry: *mut c_void, mask: *mut u64) -> libc::c_int;
        fn acl_free(acl: *mut c_void) -> libc::c_int;
    }

    // Darwin allows write access through an ALLOW ACE independent of mode
    // bits. Read/search and deny ACEs (including a system "deny delete") are safe.
    const MUTATION: u64 =
        (1 << 2) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 8) | (1 << 10) | (1 << 12) | (1 << 13);

    // SAFETY: the descriptor stays live, and a successful call owns an ACL.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) }; // ACL_TYPE_EXTENDED
    if acl.is_null() {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTSUP) => Ok(()),
            _ => Err(err),
        };
    }
    let mut entry = ptr::null_mut();
    let mut index = 0; // ACL_FIRST_ENTRY
    let result = loop {
        // SAFETY: ACL is valid and `entry` is a writable out parameter.
        if unsafe { acl_get_entry(acl, index, &mut entry) } != 0 {
            let err = io::Error::last_os_error();
            // Darwin reports EINVAL when there is no entry at that index.
            break if err.raw_os_error() == Some(libc::EINVAL) {
                Ok(())
            } else {
                Err(err)
            };
        }
        index = -1; // ACL_NEXT_ENTRY
        let mut tag = 0;
        let mut mask = 0;
        // SAFETY: `entry` belongs to the live ACL; both out parameters are valid.
        if unsafe {
            acl_get_tag_type(entry, &mut tag) != 0 || acl_get_permset_mask_np(entry, &mut mask) != 0
        } {
            break Err(io::Error::last_os_error());
        }
        // ACL_EXTENDED_ALLOW
        if tag == 1 && mask & MUTATION != 0 {
            break Err(unsafe_path(
                "operator workflow paths must not grant write access through ACLs",
            ));
        }
    };
    // SAFETY: the allocated ACL and its borrowed entries are no longer used.
    unsafe { acl_free(acl) };
    result
}

impl Directory {
    fn new(file: File, operator: bool) -> io::Result<Self> {
        if operator {
            require_protected(&file)?;
        }
        Ok(Self { file, operator })
    }

    pub(super) fn project_root(cwd: &Path) -> io::Result<Self> {
        // The caller chooses its working directory. Only components beneath
        // that starting directory use no-follow resolution.
        let file = open_file(open(
            cwd,
            OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
            Mode::empty(),
        ))?;
        Self::new(file, false)
    }

    pub(super) fn operator_root() -> io::Result<Option<Self>> {
        let root = Self::new(open_file(open("/", DIR_FLAGS, Mode::empty()))?, true)?;
        // Darwin's /etc is a system symlink to /private/etc. Start from the
        // real path without allowing arbitrary symlinks in the registry.
        #[cfg(target_os = "macos")]
        let root = root
            .open_dir(OsStr::new("private"), false)?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "system /private directory missing")
            })?;
        let Some(etc) = root.open_dir(OsStr::new("etc"), false)? else {
            return Ok(None);
        };
        let Some(agent) = etc.open_dir(OsStr::new("prime-agent"), false)? else {
            return Ok(None);
        };
        agent.open_dir(OsStr::new("workflows"), false)
    }

    fn fd(&self) -> Option<libc::c_int> {
        Some(self.file.as_raw_fd())
    }

    pub(super) fn open_dir(&self, name: &OsStr, create: bool) -> io::Result<Option<Self>> {
        let open = || openat(self.fd(), name, DIR_FLAGS, Mode::empty());
        let fd = match open() {
            Err(Errno::ENOENT) if create => {
                match mkdirat(self.fd(), name, Mode::from_bits_truncate(0o755)) {
                    Err(Errno::EEXIST) | Ok(()) => {}
                    Err(err) => return Err(err.into()),
                }
                open()?
            }
            Err(Errno::ENOENT) => return Ok(None),
            result => result?,
        };
        Self::new(open_file(Ok(fd))?, self.operator).map(Some)
    }

    fn stat(&self, name: &OsStr) -> io::Result<Option<FileStat>> {
        match fstatat(self.fd(), name, AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(Some(stat)),
            Err(Errno::ENOENT) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) fn contains(&self, name: &OsStr) -> io::Result<bool> {
        self.stat(name).map(|stat| stat.is_some())
    }

    pub(super) fn open_file(&self, name: &OsStr) -> io::Result<Option<File>> {
        let Some(stat) = self.stat(name)? else {
            return Ok(None);
        };
        if SFlag::from_bits_truncate(stat.st_mode) != SFlag::S_IFREG {
            return Err(unsafe_path(
                "workflow entries must be regular files, not links",
            ));
        }
        // Inspect the opened file as well, in case a project path was swapped
        // after stat. NONBLOCK ensures racing FIFOs cannot hang the reader.
        let file = open_file(openat(
            self.fd(),
            name,
            OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK,
            Mode::empty(),
        ))?;
        if !file.metadata()?.is_file() {
            return Err(unsafe_path("workflow entries must be regular files"));
        }
        if self.operator {
            require_protected(&file)?;
        }
        Ok(Some(file))
    }

    pub(super) fn publish(&self, name: &OsStr, contents: &[u8]) -> io::Result<()> {
        // Resolve every mutation relative to the opened directory, never
        // to a path an attacker can swap for a symbolic link. Hard-link
        // publication is atomic and refuses every existing destination.
        let temporary = format!(".workflow-{}.tmp", uuid::Uuid::new_v4());
        let temporary = OsStr::new(&temporary);
        let mut file = open_file(openat(
            self.fd(),
            temporary,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o644),
        ))?;
        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            linkat(self.fd(), temporary, self.fd(), name, AtFlags::empty())?;
            Ok(())
        })();
        if let Err(err) = unlinkat(self.fd(), temporary, UnlinkatFlags::NoRemoveDir) {
            tracing::warn!(%err, "could not remove workflow staging file");
        }
        result
    }

    pub(super) fn for_each_file(&self, mut visit: impl FnMut(&OsStr)) -> io::Result<()> {
        for entry in Dir::openat(self.fd(), ".", DIR_FLAGS, Mode::empty())? {
            let entry = entry?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if self
                .stat(name)?
                .is_some_and(|stat| SFlag::from_bits_truncate(stat.st_mode) == SFlag::S_IFREG)
            {
                visit(name);
            }
        }
        Ok(())
    }
}
