use std::ffi::{c_void, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem::{offset_of, size_of, size_of_val};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::{ptr, slice};

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_SHARE_ALL: u32 = 7;
const FILE_LIST_DIRECTORY: u32 = 1;
const FILE_TRAVERSE: u32 = 0x20;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const DELETE: u32 = 0x1_0000;
const READ_CONTROL: u32 = 0x2_0000;
const SYNCHRONIZE: u32 = 0x10_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const FILE_OPEN_IF: u32 = 3;
const FILE_DIRECTORY_FILE: u32 = 1;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_OPEN_REPARSE_POINT: u32 = 0x20_0000;
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const OBJ_DONT_REPARSE: u32 = 0x1000;
const DIR_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: RawHandle,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_quality_of_service: *const c_void,
}

#[repr(C)]
#[derive(Default)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
}

#[repr(C)]
struct RenameInfo {
    replace_if_exists: u8,
    root_directory: RawHandle,
    filename_length: u32,
    // A definition name is at most 64 ASCII characters plus `.json`.
    filename: [u16; 69],
}

#[repr(C)]
struct DirectoryInfo {
    next_entry_offset: u32,
    file_index: u32,
    creation_time: i64,
    last_access_time: i64,
    last_write_time: i64,
    change_time: i64,
    end_of_file: i64,
    allocation_size: i64,
    file_attributes: u32,
    filename_length: u32,
    ea_size: u32,
    filename: [u16; 1],
}

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

#[repr(C)]
struct Acl {
    revision: u8,
    reserved: u8,
    size: u16,
    ace_count: u16,
    reserved2: u16,
}

#[repr(C)]
struct AccessAce {
    kind: u8,
    flags: u8,
    size: u16,
    mask: u32,
    sid_start: u32,
}

#[repr(C)]
struct Sid<const N: usize> {
    revision: u8,
    subauthority_count: u8,
    authority: [u8; 6],
    subauthorities: [u32; N],
}

#[link(name = "ntdll")]
extern "system" {
    fn NtCreateFile(
        file: *mut RawHandle,
        access: u32,
        attributes: *const ObjectAttributes,
        status: *mut IoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share_access: u32,
        disposition: u32,
        options: u32,
        ea_buffer: *const c_void,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file: RawHandle,
        status: *mut IoStatusBlock,
        information: *const c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetFileInformationByHandleEx(
        file: RawHandle,
        class: u32,
        buffer: *mut c_void,
        size: u32,
    ) -> i32;
    fn LocalFree(ptr: *mut c_void) -> *mut c_void;
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetKnownFolderPath(
        id: *const Guid,
        flags: u32,
        token: RawHandle,
        path: *mut *mut u16,
    ) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(ptr: *mut c_void);
}

#[link(name = "advapi32")]
extern "system" {
    fn GetSecurityInfo(
        handle: RawHandle,
        object_type: u32,
        security_info: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *const Acl,
        sacl: *mut *const Acl,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetAce(acl: *const Acl, index: u32, ace: *mut *const AccessAce) -> i32;
    fn EqualSid(left: *const c_void, right: *const c_void) -> i32;
}

struct SecurityDescriptor(*mut c_void);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: GetSecurityInfo allocated this security descriptor; null is allowed.
        unsafe { LocalFree(self.0) };
    }
}

fn trusted_sid(sid: *const c_void) -> bool {
    const AUTHORITY_NT: [u8; 6] = [0, 0, 0, 0, 0, 5];
    const SYSTEM: Sid<1> = Sid {
        revision: 1,
        subauthority_count: 1,
        authority: AUTHORITY_NT,
        subauthorities: [18],
    };
    const ADMINISTRATORS: Sid<2> = Sid {
        revision: 1,
        subauthority_count: 2,
        authority: AUTHORITY_NT,
        subauthorities: [32, 544],
    };
    const TRUSTED_INSTALLER: Sid<6> = Sid {
        revision: 1,
        subauthority_count: 6,
        authority: AUTHORITY_NT,
        subauthorities: [
            80,
            956_008_885,
            3_418_522_649,
            1_831_038_044,
            1_853_292_631,
            2_271_478_464,
        ],
    };
    !sid.is_null()
        // SAFETY: the caller supplies a SID from a system-validated security
        // descriptor. The other SIDs above use the native SID layout.
        && unsafe {
            EqualSid(sid, ptr::from_ref(&SYSTEM).cast()) != 0
                || EqualSid(sid, ptr::from_ref(&ADMINISTRATORS).cast()) != 0
                || EqualSid(sid, ptr::from_ref(&TRUSTED_INSTALLER).cast()) != 0
        }
}

fn unsafe_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn require_protected(file: &File) -> io::Result<()> {
    const OWNER_AND_DACL_SECURITY_INFORMATION: u32 = 5;
    const SE_FILE_OBJECT: u32 = 1;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;
    const INHERIT_ONLY_ACE: u8 = 8;
    // Generic all/write, delete, write DAC/owner, and write data, attributes,
    // extended attributes, append data, and delete children (directories).
    const MUTATION: u32 = 0x1000_0000 | GENERIC_WRITE | DELETE | 0xC_0000 | 0x156;

    let mut descriptor = SecurityDescriptor(ptr::null_mut());
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null();
    // SAFETY: all requested out pointers are writable; the returned owner and
    // DACL are borrowed from the allocated descriptor until it is freed.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_AND_DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor.0,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let denied = || {
        unsafe_path("operator workflow paths must be administrator-owned and not writable by ordinary users")
    };
    if !trusted_sid(owner) || dacl.is_null() {
        return Err(denied());
    }
    // SAFETY: the descriptor owns a valid, non-null ACL.
    let count = unsafe { (*dacl).ace_count };
    for index in 0..u32::from(count) {
        let mut ace = ptr::null();
        // SAFETY: `index` is bounded by the ACL's count, and `ace` is writable.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: all ACEs start with a header containing `kind` and `flags`.
        let (kind, flags) = unsafe { ((*ace).kind, (*ace).flags) };
        if flags & INHERIT_ONLY_ACE != 0 || kind == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if kind != ACCESS_ALLOWED_ACE_TYPE {
            return Err(denied());
        }
        // SAFETY: a basic ACCESS_ALLOWED_ACE has a mask and embedded SID.
        if unsafe {
            (*ace).mask & MUTATION != 0 && !trusted_sid(ptr::addr_of!((*ace).sid_start).cast())
        } {
            return Err(denied());
        }
    }
    Ok(())
}

fn program_files() -> io::Result<PathBuf> {
    const PROGRAM_FILES: Guid = Guid(
        0x905e_63b6,
        0xc1bf,
        0x494e,
        [0xb2, 0x9c, 0x65, 0xb7, 0x32, 0xd3, 0xd2, 0x1a],
    );
    let mut path = ptr::null_mut();
    // SAFETY: the GUID and out pointer are valid. This looks up the OS-known
    // Program Files location, never a caller-controlled environment variable.
    let status = unsafe { SHGetKnownFolderPath(&PROGRAM_FILES, 0, ptr::null_mut(), &mut path) };
    if status < 0 {
        // SAFETY: the API requires releasing its output even on failure.
        unsafe { CoTaskMemFree(path.cast()) };
        return Err(io::Error::from_raw_os_error(status));
    }
    // SAFETY: success provides a NUL-terminated string owned by the caller.
    let result = unsafe {
        let mut len = 0;
        while *path.add(len) != 0 {
            len += 1;
        }
        let result = OsString::from_wide(slice::from_raw_parts(path, len));
        CoTaskMemFree(path.cast());
        result
    };
    Ok(PathBuf::from(result))
}

fn nt_result(status: i32) -> io::Result<()> {
    if status < 0 {
        // SAFETY: the conversion accepts any NTSTATUS value.
        Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ))
    } else {
        Ok(())
    }
}

// A native relative open uses the already-open parent handle and refuses to
// traverse any reparse point. An attacker cannot redirect an operation by
// swapping/renaming pathname ancestors between a check and a write.
fn open_relative(
    parent: &File,
    name: &OsStr,
    access: u32,
    disposition: u32,
    options: u32,
) -> io::Result<File> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let length = u16::try_from(wide.len() * size_of::<u16>()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "workflow path component is too long",
        )
    })?;
    let name = UnicodeString {
        length,
        maximum_length: length,
        buffer: wide.as_ptr(),
    };
    let attrs = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &name,
        attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        security_descriptor: ptr::null(),
        security_quality_of_service: ptr::null(),
    };
    let mut handle = ptr::null_mut();
    let mut status = IoStatusBlock::default();
    // SAFETY: all structures match their native layouts, pointers remain
    // valid for the synchronous call, and the file name is length-delimited.
    nt_result(unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attrs,
            &mut status,
            ptr::null(),
            0,
            if disposition == FILE_CREATE {
                0
            } else {
                FILE_SHARE_ALL
            },
            disposition,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    })?;
    // SAFETY: a successful call returns a new handle owned by us.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn optional(file: io::Result<File>) -> io::Result<Option<File>> {
    match file {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(super) struct Directory {
    file: File,
    operator: bool,
}

impl Directory {
    fn new(file: File, operator: bool) -> io::Result<Self> {
        let attrs = file.metadata()?.file_attributes();
        if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 || attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(unsafe_path(
                "workflow directories must not be reparse points",
            ));
        }
        if operator {
            require_protected(&file)?;
        }
        Ok(Self { file, operator })
    }

    fn open_root(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .access_mode(DIR_ACCESS | SYNCHRONIZE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        Self::new(file, false)
    }

    pub(super) fn project_root(cwd: &Path) -> io::Result<Self> {
        // The caller chooses its cwd, which may itself be reached via a link.
        Self::open_root(&cwd.canonicalize()?)
    }

    pub(super) fn operator_root() -> io::Result<Option<Self>> {
        let path = program_files()?;
        let invalid_path = || unsafe_path("invalid Program Files known folder path");
        if !path.is_absolute() {
            return Err(invalid_path());
        }
        let mut ancestors: Vec<_> = path.ancestors().collect();
        let mut root = Self::open_root(ancestors.pop().ok_or_else(invalid_path)?)?;
        for ancestor in ancestors.into_iter().rev() {
            let name = ancestor.file_name().ok_or_else(invalid_path)?;
            root = root.open_dir(name, false)?.ok_or_else(invalid_path)?;
        }
        // The known folder is the administrative trust anchor; validate it
        // and the owner/DACL of each registry directory or file beneath it.
        let program_files = Self::new(root.file, true)?;
        let Some(agent) = program_files.open_dir(OsStr::new("Beehive"), false)? else {
            return Ok(None);
        };
        agent.open_dir(OsStr::new("workflows"), false)
    }

    pub(super) fn open_dir(&self, name: &OsStr, create: bool) -> io::Result<Option<Self>> {
        let disposition = if create { FILE_OPEN_IF } else { FILE_OPEN };
        optional(open_relative(
            &self.file,
            name,
            DIR_ACCESS,
            disposition,
            FILE_DIRECTORY_FILE,
        ))?
        .map(|file| Self::new(file, self.operator))
        .transpose()
    }

    pub(super) fn contains(&self, name: &OsStr) -> io::Result<bool> {
        optional(open_relative(
            &self.file,
            name,
            FILE_READ_ATTRIBUTES,
            FILE_OPEN,
            0,
        ))
        .map(|file| file.is_some())
    }

    pub(super) fn open_file(&self, name: &OsStr) -> io::Result<Option<File>> {
        let Some(file) = optional(open_relative(
            &self.file,
            name,
            GENERIC_READ,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        ))?
        else {
            return Ok(None);
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(unsafe_path(
                "workflow entries must be regular files, not reparse points",
            ));
        }
        if self.operator {
            require_protected(&file)?;
        }
        Ok(Some(file))
    }

    pub(super) fn publish(&self, name: &OsStr, contents: &[u8]) -> io::Result<()> {
        let temporary = format!(".workflow-{}.tmp", uuid::Uuid::new_v4());
        let mut file = open_relative(
            &self.file,
            OsStr::new(&temporary),
            GENERIC_WRITE | DELETE,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
        )?;
        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            let mut renaming = RenameInfo {
                replace_if_exists: 0,
                // With a simple name and null root, rename stays in the source directory.
                root_directory: ptr::null_mut(),
                filename_length: 0,
                filename: [0; 69],
            };
            for (slot, unit) in renaming.filename.iter_mut().zip(name.encode_wide()) {
                *slot = unit;
                renaming.filename_length += size_of::<u16>() as u32;
            }
            set_info(&file, &renaming, 10) // FileRenameInformation
        })();
        if result.is_err() {
            // Delete the staging file by handle, not a possibly swapped name.
            if let Err(err) = set_info(&file, &1_u8, 13) {
                // FileDispositionInformation
                tracing::warn!(%err, "could not remove workflow staging file");
            }
        }
        result
    }

    pub(super) fn for_each_file(&self, mut visit: impl FnMut(&OsStr)) -> io::Result<()> {
        const FILE_FULL_DIRECTORY_INFO: u32 = 14;
        const FILE_FULL_DIRECTORY_RESTART_INFO: u32 = 15;
        const ERROR_NO_MORE_FILES: i32 = 18;
        const HEADER_SIZE: usize = offset_of!(DirectoryInfo, filename);
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid workflow directory entry",
            )
        };
        let mut buffer = [0_u64; 1024];
        let mut class = FILE_FULL_DIRECTORY_RESTART_INFO;
        loop {
            // SAFETY: the handle is a directory and the buffer is writable,
            // aligned, and at least `size_of_val` bytes long.
            let success = unsafe {
                GetFileInformationByHandleEx(
                    self.file.as_raw_handle(),
                    class,
                    buffer.as_mut_ptr().cast(),
                    size_of_val(&buffer) as u32,
                )
            };
            if success == 0 {
                let err = io::Error::last_os_error();
                return if err.raw_os_error() == Some(ERROR_NO_MORE_FILES) {
                    Ok(())
                } else {
                    Err(err)
                };
            }
            class = FILE_FULL_DIRECTORY_INFO;
            let mut offset = 0;
            loop {
                if offset + size_of::<DirectoryInfo>() > size_of_val(&buffer) {
                    return Err(invalid());
                }
                // SAFETY: `offset` was bounds checked and all struct fields
                // are integers, with every bit pattern valid.
                let entry = unsafe {
                    ptr::read_unaligned(
                        buffer
                            .as_ptr()
                            .cast::<u8>()
                            .add(offset)
                            .cast::<DirectoryInfo>(),
                    )
                };
                let length = entry.filename_length as usize;
                if length % 2 != 0 || offset + HEADER_SIZE + length > size_of_val(&buffer) {
                    return Err(invalid());
                }
                if entry.file_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                    == 0
                {
                    // SAFETY: offsets are aligned and the filename fits the buffer.
                    let wide = unsafe {
                        slice::from_raw_parts(
                            buffer
                                .as_ptr()
                                .cast::<u8>()
                                .add(offset + HEADER_SIZE)
                                .cast::<u16>(),
                            length / 2,
                        )
                    };
                    visit(&OsString::from_wide(wide));
                }
                if entry.next_entry_offset == 0 {
                    break;
                }
                if entry.next_entry_offset % 8 != 0
                    || (entry.next_entry_offset as usize) < HEADER_SIZE + length
                {
                    return Err(invalid());
                }
                offset += entry.next_entry_offset as usize;
            }
        }
    }
}

fn set_info<T>(file: &File, info: &T, class: u32) -> io::Result<()> {
    let mut status = IoStatusBlock::default();
    // SAFETY: the caller chooses the information class matching `T`, and
    // the file and the information buffer stay live for the synchronous call.
    nt_result(unsafe {
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            ptr::from_ref(info).cast(),
            size_of::<T>() as u32,
            class,
        )
    })
}
