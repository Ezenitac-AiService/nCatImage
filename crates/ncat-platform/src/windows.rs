//! Windows ABI boundary. Every returned handle owns its lifetime; no FFI pointer
//! escapes a call. This provides containment/private storage, not a filesystem sandbox.
use ncat_protocol::{ErrorCode, WORKER_ADDRESS_SPACE};
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io::{self, Read, Write},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    process::Child,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Diagnostics::Debug::*, JobObjects::*, Threading::*},
};

const START: &[u8; 8] = b"NCATJOB1";
const REQUIRED: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    | JOB_OBJECT_LIMIT_PROCESS_MEMORY
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;

pub fn name_bytes(name: &OsStr) -> &[u8] {
    name.as_encoded_bytes()
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "native NUL"));
    }
    value.push(0);
    Ok(value)
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful Create/Open returns a new, non-inherited owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: all Local values are LocalAlloc-allocated API results.
        unsafe {
            LocalFree(self.0);
        }
    }
}

struct User {
    _token: OwnedHandle,
    storage: Vec<usize>,
}
impl User {
    fn current() -> io::Result<Self> {
        // SAFETY: sized/aligned storage is kept alive together with the SID pointers.
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = owned(token)?;
            let mut bytes = 0;
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut bytes,
            );
            if bytes == 0 || bytes > 65536 {
                return Err(io::Error::other("token information size"));
            }
            let mut storage = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                storage.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                _token: token,
                storage,
            })
        }
    }
    fn sid(&self) -> PSID {
        // SAFETY: GetTokenInformation(TokenUser) initialized this aligned allocation.
        unsafe { (*(self.storage.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }
}

fn descriptor() -> io::Result<Local> {
    let user = User::current()?;
    // SAFETY: user storage remains live until the SID string is copied.
    unsafe {
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(user.sid(), &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _text = Local(text.cast());
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
            if length > 512 {
                return Err(io::Error::other("SID string size"));
            }
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(text, length))
            .map_err(io::Error::other)?;
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut sd = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut sd,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Local(sd))
    }
}

fn attributes(sd: &Local) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    }
}

fn open_no_reparse(path: &Path, access: u32, directory: bool) -> io::Result<File> {
    let name = wide(path)?;
    // SAFETY: buffers live through CreateFile; ownership transfers into File.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            ptr::null_mut(),
        )
    };
    let handle = owned(handle)?;
    let file = File::from(handle);
    use std::os::windows::fs::MetadataExt;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reparse or incorrect file type",
        ));
    }
    Ok(file)
}

fn verify_private(file: &File) -> io::Result<()> {
    let user = User::current()?;
    // SAFETY: GetSecurityInfo's descriptor owns the returned ACL/SID pointers.
    unsafe {
        let mut owner = ptr::null_mut();
        let mut acl: *mut ACL = ptr::null_mut();
        let mut sd = ptr::null_mut();
        let code = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut acl,
            ptr::null_mut(),
            &mut sd,
        );
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let _sd = Local(sd);
        if owner.is_null()
            || EqualSid(owner, user.sid()) == 0
            || acl.is_null()
            || (*acl).AceCount == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private owner/DACL required",
            ));
        }
        for index in 0..u32::from((*acl).AceCount) {
            let mut ace = ptr::null_mut();
            if GetAce(acl, index, &mut ace) == 0 {
                return Err(io::Error::last_os_error());
            }
            if (*(ace.cast::<ACE_HEADER>())).AceType != 0 {
                // ACCESS_ALLOWED_ACE_TYPE
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unexpected private ACE",
                ));
            }
            let sid = ptr::addr_of!((*(ace.cast::<ACCESS_ALLOWED_ACE>())).SidStart)
                .cast_mut()
                .cast();
            if IsValidSid(sid) == 0
                || (EqualSid(sid, user.sid()) == 0 && IsWellKnownSid(sid, WinLocalSystemSid) == 0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "non-private ACE mask={} admin={} world={}",
                        (*(ace.cast::<ACCESS_ALLOWED_ACE>())).Mask,
                        IsWellKnownSid(sid, WinBuiltinAdministratorsSid),
                        IsWellKnownSid(sid, WinWorldSid)
                    ),
                ));
            }
        }
    }
    Ok(())
}

pub fn create_private_directory(path: &Path) -> io::Result<()> {
    let name = wide(path)?;
    let sd = descriptor()?;
    // SAFETY: SECURITY_ATTRIBUTES references the owned descriptor for the call.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes(&sd)) } == 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        verify_private(&open_no_reparse(path, READ_CONTROL, true)?)?;
        return Err(error);
    }
    verify_private(&open_no_reparse(path, READ_CONTROL, true)?)
}

pub fn create_private_file(path: &Path) -> io::Result<File> {
    let name = wide(path)?;
    let sd = descriptor()?;
    // SAFETY: API returns a new File handle; CREATE_NEW never follows/replaces an existing entry.
    let file = File::from(owned(unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes(&sd),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    })?);
    verify_private(&file)?;
    Ok(file)
}

pub fn open_pixel_output(path: &Path) -> io::Result<File> {
    let file = open_no_reparse(path, GENERIC_WRITE | READ_CONTROL, false)?;
    verify_private(&file)?;
    // Truncate only AFTER handle-based type/reparse/DACL verification.
    file.set_len(0)?;
    Ok(file)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub device: u64,
    pub inode: u64,
    pub identity_high: u64,
    pub size: u64,
    pub modified: (i64, i64),
    pub changed: (i64, i64),
}
impl Revision {
    pub fn read(path: &Path) -> Result<Self, ErrorCode> {
        Self::of(&File::open(path).map_err(|_| ErrorCode::Io)?)
    }
    pub fn of(file: &File) -> Result<Self, ErrorCode> {
        let metadata = file.metadata().map_err(|_| ErrorCode::Io)?;
        if !metadata.is_file() {
            return Err(ErrorCode::Unsupported);
        }
        // SAFETY: typed, initialized output structs and valid owned file handle.
        unsafe {
            let mut id: FILE_ID_INFO = zeroed();
            let mut basic: FILE_BASIC_INFO = zeroed();
            if GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            ) == 0
                || GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&mut basic as *mut FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                ) == 0
            {
                return Err(ErrorCode::Io);
            }
            let bytes = id.FileId.Identifier;
            Ok(Self {
                device: id.VolumeSerialNumber,
                inode: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                identity_high: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
                size: metadata.len(),
                modified: (basic.LastWriteTime, 0),
                changed: (basic.ChangeTime, 0),
            })
        }
    }
    pub fn differences(&self, now: &Self) -> String {
        format!(
            "stage=revision_comparison identity_changed={} size_changed={} mtime_changed={} ctime_changed={}",
            (self.device, self.inode, self.identity_high)
                != (now.device, now.inode, now.identity_high),
            self.size != now.size,
            self.modified != now.modified,
            self.changed != now.changed
        )
    }
}

pub struct WorkerProtection {
    _job: OwnedHandle,
}
pub fn start_protected_worker(child: &mut Child) -> io::Result<WorkerProtection> {
    // SAFETY: owned non-inherited Job handle remains alive until the worker is reaped.
    unsafe {
        let job = owned(CreateJobObjectW(ptr::null(), ptr::null()))?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags =
            REQUIRED | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = WORKER_ADDRESS_SPACE as usize;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
            || AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut verified: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        if QueryInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&mut verified as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ptr::null_mut(),
        ) == 0
            || verified.BasicLimitInformation.LimitFlags & REQUIRED != REQUIRED
            || verified.ProcessMemoryLimit != WORKER_ADDRESS_SPACE as usize
            || verified.BasicLimitInformation.ActiveProcessLimit != 1
        {
            return Err(io::Error::other(format!(
                "Job readback unavailable flags={} memory={}",
                verified.BasicLimitInformation.LimitFlags, verified.ProcessMemoryLimit
            )));
        }
        child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("worker pipe"))?
            .write_all(START)?;
        Ok(WorkerProtection { _job: job })
    }
}

pub fn protect_worker() -> io::Result<()> {
    if std::env::var_os("NCAT_PROTECTED_WORKER").as_deref() != Some(OsStr::new("1")) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "protected launch required",
        ));
    }
    let mut start = [0; 8];
    std::io::stdin().lock().read_exact(&mut start)?;
    if &start != START {
        return Err(io::Error::other("worker start handshake"));
    }
    // SAFETY: NULL job queries the current process job after parent assignment.
    unsafe {
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        if QueryInformationJobObject(
            ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        if limits.BasicLimitInformation.LimitFlags & REQUIRED != REQUIRED
            || limits.ProcessMemoryLimit == 0
            || limits.ProcessMemoryLimit > WORKER_ADDRESS_SPACE as usize
            || limits.BasicLimitInformation.ActiveProcessLimit != 1
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "worker containment unavailable flags={} memory={} processes={}",
                    limits.BasicLimitInformation.LimitFlags,
                    limits.ProcessMemoryLimit,
                    limits.BasicLimitInformation.ActiveProcessLimit
                ),
            ));
        }
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn private_output_is_create_new_and_handle_checked() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        create_private_directory(&directory).unwrap();
        assert_eq!(
            create_private_directory(&directory).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let path = directory.join("한글 pixels");
        let mut owner = create_private_file(&path).unwrap();
        owner.write_all(b"original pixels").unwrap();
        assert!(create_private_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original pixels");
        assert!(open_pixel_output(&directory).is_err());
        assert!(open_pixel_output(&directory.join("missing")).is_err());
        open_pixel_output(&path).unwrap();
        assert_eq!(owner.metadata().unwrap().len(), 0);
    }

    #[test]
    fn replacing_path_does_not_replace_open_file_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("page");
        fs::write(&path, b"old").unwrap();
        let owner = File::open(&path).unwrap();
        let old = Revision::of(&owner).unwrap();
        fs::rename(&path, root.path().join("previous")).unwrap();
        fs::write(&path, b"new").unwrap();
        let now = Revision::read(&path).unwrap();
        assert_ne!(
            (old.device, old.inode, old.identity_high),
            (now.device, now.inode, now.identity_high)
        );
        let retained = Revision::of(&owner).unwrap();
        assert_eq!(
            (old.device, old.inode, old.identity_high),
            (retained.device, retained.inode, retained.identity_high)
        );
        assert!(Revision::read(root.path()).is_err());
    }

    #[test]
    #[ignore = "native Windows Developer Mode or symlink privilege required; run explicitly"]
    fn final_reparse_is_rejected_before_truncation() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        create_private_directory(&directory).unwrap();
        let path = directory.join("pixels");
        let mut owner = create_private_file(&path).unwrap();
        owner.write_all(b"keep").unwrap();
        let link = directory.join("link");
        std::os::windows::fs::symlink_file(&path, &link).unwrap();
        assert!(open_pixel_output(&link).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }
}
