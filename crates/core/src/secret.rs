//! Keeping keys out of plain sight on disk.
//!
//! On Windows a secret is sealed with the data protection API, tied to the
//! Windows account: copying the file to another machine or account gives
//! nothing. Elsewhere (development and tests) it is stored as it is.

#[cfg(windows)]
mod imp {
    use std::ptr;

    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(input: *const Blob, description: *const u16, entropy: *const Blob, reserved: *mut core::ffi::c_void, prompt: *const core::ffi::c_void, flags: u32, output: *mut Blob) -> i32;
        fn CryptUnprotectData(input: *const Blob, description: *mut *mut u16, entropy: *const Blob, reserved: *mut core::ffi::c_void, prompt: *const core::ffi::c_void, flags: u32, output: *mut Blob) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    /// Never show a window, even if Windows thinks it should ask.
    const UI_FORBIDDEN: u32 = 0x1;

    fn take(out: Blob) -> Vec<u8> {
        // SAFETY: on success Windows hands us `len` bytes at `data`, which we
        // copy and then free with the function the API documents.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.data, out.len as usize).to_vec();
            LocalFree(out.data.cast());
            bytes
        }
    }

    pub fn seal(secret: &[u8]) -> Option<Vec<u8>> {
        let input = Blob { len: secret.len() as u32, data: secret.as_ptr().cast_mut() };
        let mut out = Blob { len: 0, data: ptr::null_mut() };
        // SAFETY: both blobs are valid for the call; the input is not written.
        let ok = unsafe { CryptProtectData(&input, ptr::null(), ptr::null(), ptr::null_mut(), ptr::null(), UI_FORBIDDEN, &mut out) };
        (ok != 0).then(|| take(out))
    }

    pub fn open(sealed: &[u8]) -> Option<Vec<u8>> {
        let input = Blob { len: sealed.len() as u32, data: sealed.as_ptr().cast_mut() };
        let mut out = Blob { len: 0, data: ptr::null_mut() };
        // SAFETY: as above.
        let ok = unsafe { CryptUnprotectData(&input, ptr::null_mut(), ptr::null(), ptr::null_mut(), ptr::null(), UI_FORBIDDEN, &mut out) };
        (ok != 0).then(|| take(out))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn seal(secret: &[u8]) -> Option<Vec<u8>> {
        Some(secret.to_vec())
    }
    pub fn open(sealed: &[u8]) -> Option<Vec<u8>> {
        Some(sealed.to_vec())
    }
}

/// Seal a secret for storing on disk.
pub fn seal(secret: &[u8]) -> Result<Vec<u8>, String> {
    imp::seal(secret).ok_or_else(|| "Windows could not protect a key.".to_string())
}

/// Open a sealed secret. Fails when it was sealed by another Windows account.
pub fn open(sealed: &[u8]) -> Result<Vec<u8>, String> {
    imp::open(sealed).ok_or_else(|| "A stored key could not be read. Link the desktop again.".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_sealed_secret_opens_again() {
        let secret = b"thirty-two bytes of device key!!";
        let sealed = super::seal(secret).unwrap();
        #[cfg(windows)]
        assert_ne!(sealed, secret, "sealed with the Windows account");
        assert_eq!(super::open(&sealed).unwrap(), secret);
        #[cfg(windows)]
        assert!(super::open(b"not a sealed secret").is_err());
    }
}
