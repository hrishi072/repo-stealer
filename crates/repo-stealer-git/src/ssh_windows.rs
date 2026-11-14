//! Create the staged key with a protected current-user DACL from the outset.
//! EFS inheritance is still provided by the verified workspace directory.
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::Write,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, LocalFree, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    },
    Security::{
        Authorization::{
            SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
        },
        GetTokenInformation, InitializeSecurityDescriptor, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, TokenUser, SECURITY_ATTRIBUTES,
        SECURITY_DESCRIPTOR, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
    },
    Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalAllocation(*mut core::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(super) fn write_key(path: &Path, contents: &[u8]) -> Result<()> {
    // SAFETY: Every API receives valid aligned, live storage. The token-user
    // buffer owns its SID until CreateFileW copies the descriptor; the ACL is
    // freed with LocalFree, and the returned file handle is transferred once.
    unsafe {
        let mut token = Token(ptr::null_mut());
        ensure!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token.0) != 0,
            "Cannot identify the Windows SSH key owner"
        );
        let mut length = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut length);
        ensure!(
            (std::mem::size_of::<TOKEN_USER>() as u32..=65536).contains(&length),
            "Invalid Windows token-user size"
        );
        let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        ensure!(
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length
            ) != 0,
            "Cannot read the Windows SSH key owner"
        );
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: 0,
            ..Default::default()
        };
        entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
        entry.Trustee.TrusteeType = TRUSTEE_IS_USER;
        entry.Trustee.ptstrName = user.User.Sid.cast();
        let mut acl = ptr::null_mut();
        ensure!(
            SetEntriesInAclW(1, &entry, ptr::null(), &mut acl) == 0,
            "Cannot create private Windows SSH permissions"
        );
        let _acl = LocalAllocation(acl.cast());
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let security = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        ensure!(
            InitializeSecurityDescriptor(security, 1) != 0
                && SetSecurityDescriptorOwner(security, user.User.Sid, 0) != 0
                && SetSecurityDescriptorDacl(security, 1, acl, 0) != 0
                && SetSecurityDescriptorControl(security, SE_DACL_PROTECTED, SE_DACL_PROTECTED)
                    != 0,
            "Cannot configure private Windows SSH permissions"
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security,
            bInheritHandle: 0,
        };
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        ensure!(
            !filename[..filename.len() - 1].contains(&0),
            "Invalid Windows SSH key path"
        );
        let handle = CreateFileW(
            filename.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        );
        ensure!(
            handle != INVALID_HANDLE_VALUE,
            "Cannot create the private staged SSH key"
        );
        let mut file = File::from_raw_handle(handle);
        file.write_all(contents)
            .context("Cannot write the staged SSH key")?;
        file.sync_all().context("Cannot flush the staged SSH key")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
        EqualSid, GetAce, GetSecurityDescriptorControl, ACCESS_ALLOWED_ACE,
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    };
    #[test]
    fn staged_key_has_a_single_owner_only_protected_acl() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("identity");
        write_key(&path, b"synthetic key fixture").unwrap();
        assert!(write_key(&path, b"must not replace").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic key fixture");
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: The returned security descriptor owns every borrowed SID,
        // ACL and ACE below; it is kept live until all assertions complete.
        unsafe {
            let mut owner = ptr::null_mut();
            let mut acl = ptr::null_mut();
            let mut descriptor = ptr::null_mut();
            assert_eq!(
                GetNamedSecurityInfoW(
                    filename.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                    &mut owner,
                    ptr::null_mut(),
                    &mut acl,
                    ptr::null_mut(),
                    &mut descriptor
                ),
                0
            );
            let _descriptor = LocalAllocation(descriptor);
            assert!(!owner.is_null() && !acl.is_null());
            assert_eq!((*acl).AceCount, 1);
            let mut ace = ptr::null_mut();
            assert_ne!(GetAce(acl, 0, &mut ace), 0);
            let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(allowed.Header.AceType, 0);
            assert_eq!(allowed.Mask, FILE_ALL_ACCESS);
            assert_ne!(
                EqualSid(owner, (&allowed.SidStart as *const u32).cast_mut().cast()),
                0
            );
            let mut control = 0;
            let mut revision = 0;
            assert_ne!(
                GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
                0
            );
            assert_ne!(control & SE_DACL_PROTECTED, 0);
        }
    }
}
