//! The DACL the device's secrets live under on Windows.
//!
//! `<data>/token` (the install token), `<data>/identity.key` (the mesh private key) and
//! `<data>/console-token.json` (the console channel token) are credentials. On Unix they are created
//! 0600; on Windows a mode does not exist and a file inherits whatever DACL its directory carries.
//! The launcher used to protect only `<data>/token`; the other two inherited `<data>`'s ACL, which on
//! a fresh `D:\Summrise` is not something this code set or checked (review C1).
//!
//! This crate is the one implementation of "SYSTEM and Administrators, nobody else". It installs a
//! protected DACL that grants `FILE_ALL_ACCESS` to exactly those two principals, and it **reads the
//! descriptor back** and refuses a path whose DACL allows any other principal or does not name both.
//! A directory is protected with inheritable ACEs, so a file created in it inherits the same two
//! principals — that is what makes protecting `<data>` at install cover the files written later.
//!
//! The read-back is the part that matters. `SetNamedSecurityInfoW` reports success for a descriptor
//! that is not the one this code intended (a redirector or a hostile parent can change what applies
//! after the call), so the only answer worth acting on is the one read from the object afterwards.

#![cfg(windows)]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{LocalFree, FALSE, HLOCAL, PSID};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, AddAccessAllowedAceEx, CreateWellKnownSid, EqualSid, GetAce,
    GetAclInformation, GetSecurityDescriptorDacl, InitializeAcl, WinBuiltinAdministratorsSid,
    WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION,
    CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, WELL_KNOWN_SID_TYPE,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

/// `SECURITY_MAX_SID_SIZE`: the documented upper bound on a SID that fits in a struct.
const MAX_SID_SIZE: usize = 68;

/// An ACL built for at most two ACEs fits inside this with room to spare; the Windows ACL functions
/// require DWORD alignment, which the `align(8)` supplies.
const ACL_STORAGE_BYTES: usize = 1024;

/// A SID copied into stack storage. The Win32 calls read it; nothing here returns it.
#[repr(C, align(4))]
struct SidStorage([u8; MAX_SID_SIZE]);

/// An ACL in stack storage, valid for the duration of one `SetNamedSecurityInfoW`.
#[repr(C, align(8))]
struct AclStorage([u8; ACL_STORAGE_BYTES]);

/// Replace `path`'s DACL with a protected one granting full control to SYSTEM and Administrators.
///
/// The file itself is the object: use this after the file exists and before the secret is written to
/// it. The returned error names the path; a path that cannot be read back is a refusal, never a
/// warning.
pub fn protect_file(path: &Path) -> Result<(), String> {
    protect(path, false)
}

/// Replace `path`'s DACL with a protected one granting full control to SYSTEM and Administrators,
/// with both ACEs inheritable by the files and directories created inside it.
///
/// Protecting the directory before its children exist is what gives a later file a safe DACL without
/// every writer having to remember to call [`protect_file`].
pub fn protect_dir(path: &Path) -> Result<(), String> {
    protect(path, true)
}

/// Read `path`'s DACL back and refuse it unless SYSTEM and Administrators are the only principals.
///
/// Both must appear, and no other ACE may appear: an allowed ACE for any third principal (Users,
/// Everyone, the interactive user's own SID) is a refusal, as is a deny/audit ACE this code did not
/// write. A missing DACL — which grants everyone full control — is a refusal too. This is the check
/// [`protect_file`] and [`protect_dir`] run after hardening, exposed so a reader can refuse a file
/// that was copied or restored without protection.
pub fn verify(path: &Path) -> Result<(), String> {
    let system =
        create_sid(WinLocalSystemSid).map_err(|error| format!("{}: {error}", path.display()))?;
    let administrators = create_sid(WinBuiltinAdministratorsSid)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    verify_against(path, &system, &administrators)
}

fn protect(path: &Path, directory: bool) -> Result<(), String> {
    let system =
        create_sid(WinLocalSystemSid).map_err(|error| format!("{}: {error}", path.display()))?;
    let administrators = create_sid(WinBuiltinAdministratorsSid)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let acl = build_acl(&system, &administrators, directory)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let wide = wide(path);

    // `PROTECTED_DACL_SECURITY_INFORMATION` stops the parent directory's inheritable entries from
    // applying to this object; the two ACEs below are then the whole DACL.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl.0.as_ptr() as *const ACL,
            ptr::null(),
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot set the DACL on {}: {}",
            path.display(),
            io::Error::from_raw_os_error(status as i32)
        ));
    }

    // The call above is a request, not a fact. The fact is what the object's descriptor says now.
    verify_against(path, &system, &administrators)
}

fn verify_against(
    path: &Path,
    system: &SidStorage,
    administrators: &SidStorage,
) -> Result<(), String> {
    let wide = wide(path);
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot read the DACL of {}: {}",
            path.display(),
            io::Error::from_raw_os_error(status as i32)
        ));
    }

    // The descriptor is allocated by the call above and must be freed on every path, including the
    // refusals. The closure keeps the checks readable and the free in one place.
    let checked = (|| {
        let mut present = 0;
        let mut defaulted = 0;
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == FALSE
        {
            return Err(format!(
                "cannot read the DACL of {}: {}",
                path.display(),
                io::Error::last_os_error()
            ));
        }
        if present == FALSE || dacl.is_null() {
            return Err(format!(
                "{} has no DACL, which grants everyone full control",
                path.display()
            ));
        }

        let mut information: ACL_SIZE_INFORMATION = unsafe { zeroed() };
        if unsafe {
            GetAclInformation(
                dacl,
                &mut information as *mut ACL_SIZE_INFORMATION as *mut c_void,
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == FALSE
        {
            return Err(format!(
                "cannot read the DACL of {}: {}",
                path.display(),
                io::Error::last_os_error()
            ));
        }

        let mut saw_system = false;
        let mut saw_administrators = false;
        for index in 0..information.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut ace) } == FALSE {
                return Err(format!(
                    "cannot read ACE {index} of {}: {}",
                    path.display(),
                    io::Error::last_os_error()
                ));
            }
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                return Err(format!(
                    "{} has a DACL entry that is not an allow ACE; refusing it",
                    path.display()
                ));
            }
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let sid = &allowed.SidStart as *const u32 as PSID;
            let is_system = unsafe { EqualSid(sid, system.0.as_ptr() as PSID) } != FALSE;
            let is_administrators =
                unsafe { EqualSid(sid, administrators.0.as_ptr() as PSID) } != FALSE;
            if is_system {
                saw_system = true;
            } else if is_administrators {
                saw_administrators = true;
            } else {
                return Err(format!(
                    "{} allows a principal other than SYSTEM or BUILTIN\\Administrators; \
                     refusing it",
                    path.display()
                ));
            }
        }

        if !saw_system || !saw_administrators {
            return Err(format!(
                "{} does not grant full control to both SYSTEM and \
                 BUILTIN\\Administrators; refusing it",
                path.display()
            ));
        }
        Ok(())
    })();

    unsafe {
        LocalFree(descriptor as HLOCAL);
    }
    checked
}

fn create_sid(kind: WELL_KNOWN_SID_TYPE) -> Result<SidStorage, String> {
    let mut sid = SidStorage([0; MAX_SID_SIZE]);
    let mut size = MAX_SID_SIZE as u32;
    if unsafe { CreateWellKnownSid(kind, ptr::null_mut(), sid.0.as_mut_ptr() as PSID, &mut size) }
        == FALSE
    {
        return Err(format!(
            "CreateWellKnownSid failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(sid)
}

fn build_acl(
    system: &SidStorage,
    administrators: &SidStorage,
    directory: bool,
) -> Result<AclStorage, String> {
    let mut acl = AclStorage([0; ACL_STORAGE_BYTES]);
    if unsafe {
        InitializeAcl(
            acl.0.as_mut_ptr() as *mut ACL,
            ACL_STORAGE_BYTES as u32,
            ACL_REVISION,
        )
    } == FALSE
    {
        return Err(format!(
            "InitializeAcl failed: {}",
            io::Error::last_os_error()
        ));
    }

    // A directory's ACEs must be inheritable; without these flags a file created inside a protected
    // directory would get no DACL at all, which is the opposite of protection.
    let flags = if directory {
        OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
    } else {
        0
    };
    for sid in [system, administrators] {
        if unsafe {
            AddAccessAllowedAceEx(
                acl.0.as_mut_ptr() as *mut ACL,
                ACL_REVISION,
                flags,
                FILE_ALL_ACCESS,
                sid.0.as_ptr() as PSID,
            )
        } == FALSE
        {
            return Err(format!(
                "AddAccessAllowedAceEx failed: {}",
                io::Error::last_os_error()
            ));
        }
    }
    Ok(acl)
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A unique temp tree of this test's own. The parent directory is deliberately ordinary: the
    /// tests that prove a copy is refused need an object whose inherited DACL is not ours.
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(label: &str) -> TempTree {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "summrise-winsec-{label}-{}-{nanos}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("the test tree is creatable");
            TempTree(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// `icacls` is the operator-facing reader of the same descriptor this crate writes: the test
    /// asserts what an operator would see, not a second copy of the writer's assumptions.
    fn icacls(path: &Path) -> String {
        let output = Command::new("icacls")
            .arg(path)
            .output()
            .expect("icacls is an OS program");
        assert!(
            output.status.success(),
            "icacls {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The descriptor shows both privileged principals with full control and no other principal.
    /// `icacls` prints a directory's inheritable ACEs as `(OI)(CI)(F)`, so `(F)` is the assertion,
    /// not the exact spacing around it.
    fn assert_system_and_administrators_only(path: &Path) {
        let text = icacls(path);
        let lower = text.to_ascii_lowercase();
        assert!(
            !lower.contains("builtin\\users"),
            "{} grants BUILTIN\\Users:\n{text}",
            path.display()
        );
        for principal in ["SYSTEM:", "BUILTIN\\Administrators:"] {
            let line = text
                .lines()
                .find(|line| line.contains(principal))
                .unwrap_or_else(|| panic!("{} has no {principal} entry:\n{text}", path.display()));
            assert!(
                line.contains("(F)"),
                "{}'s {principal} entry is not full control: {line}",
                path.display()
            );
        }
    }

    #[test]
    fn a_protected_file_grants_only_system_and_administrators() {
        let tree = TempTree::new("file");
        let path = tree.join("secret.txt");
        fs::write(&path, b"the secret").expect("the fixture is written");

        protect_file(&path).expect("a file this process owns is protectable");

        assert_system_and_administrators_only(&path);
        verify(&path).expect("the read-back agrees with what was written");
        assert_eq!(
            fs::read(&path).expect("the protected file is still readable by us"),
            b"the secret",
            "protecting a file does not change its bytes"
        );
    }

    #[test]
    fn a_protected_directory_grants_only_system_and_administrators_to_its_children() {
        let tree = TempTree::new("directory");
        let directory = tree.join("data");
        fs::create_dir(&directory).expect("the directory is creatable");

        protect_dir(&directory).expect("a directory this process owns is protectable");

        assert_system_and_administrators_only(&directory);
        verify(&directory).expect("the directory's own DACL reads back");

        // A file created *after* the directory was protected must inherit the safe DACL without its
        // writer calling protect_file at all. That inheritance is the launcher's install contract.
        let child = directory.join("later.key");
        fs::write(&child, b"minted later").expect("the child is creatable");
        assert_system_and_administrators_only(&child);
        verify(&child).expect("the child inherited SYSTEM and Administrators only");
    }

    #[test]
    fn protecting_twice_is_not_an_error_and_leaves_the_same_dacl() {
        let tree = TempTree::new("twice");
        let path = tree.join("secret.txt");
        fs::write(&path, b"the secret").expect("the fixture is written");

        protect_file(&path).expect("the first protection succeeds");
        protect_file(&path).expect("an already-protected file is protectable again");

        assert_system_and_administrators_only(&path);
        verify(&path).expect("the read-back still agrees");
    }

    #[test]
    fn a_file_copied_without_protection_is_refused_by_the_read_back() {
        let protected = TempTree::new("copy-source");
        let source = protected.join("console-token.json");
        fs::write(&source, br#"{"id":"00","token":"11"}"#).expect("the fixture is written");
        protect_file(&source).expect("the source is protected");

        // The destination lands under an ordinary temp directory, so a *new* file there inherits
        // that directory's DACL — exactly the state a restored backup or a hand copy is in. Writing
        // the bytes fresh (rather than `fs::copy`, which may carry the source's descriptor) makes
        // the fixture's protection the thing under test. `verify` must refuse it.
        let unprotected = TempTree::new("copy-destination");
        let copy = unprotected.join("console-token.json");
        fs::write(
            &copy,
            fs::read(&source).expect("the protected source is readable"),
        )
        .expect("the copy is writable");

        let error = verify(&copy).expect_err("an unprotected copy is refused");
        assert!(
            error.contains(&copy.display().to_string()),
            "the refusal names the path: {error}"
        );
    }

    #[test]
    fn protecting_a_file_protects_the_secret_from_the_first_byte() {
        // The writer protects the empty file before writing the bytes (identity, install token).
        // This pins the sequence: creating then protecting then writing leaves a protected file.
        let tree = TempTree::new("before-write");
        let path = tree.join("identity.key");
        fs::write(&path, b"").expect("the empty file is creatable");
        protect_file(&path).expect("the empty file is protectable");
        fs::write(&path, b"the seed the caller writes next").expect("the writer writes the seed");

        assert_system_and_administrators_only(&path);
        verify(&path).expect("the DACL survives the later write");
    }
}
