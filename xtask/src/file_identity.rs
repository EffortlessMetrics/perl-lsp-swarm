//! Kernel-level file identity helpers shared across xtask targets.
//!
//! Hard-link-accurate identity is not available from stable `std` on Windows
//! (`windows_by_handle` metadata APIs remain unstable), so the one primitive
//! here reads it through the already-vendored `winapi` dependency instead.

use color_eyre::eyre::{Result, WrapErr};
use std::path::Path;

/// Read the kernel-level identity of an existing file on Windows.
///
/// Returns `(volume_serial_number, file_index_high, file_index_low)` obtained
/// through `GetFileInformationByHandle`. Two paths denote the same underlying
/// file exactly when all three components match, which detects hard-link
/// aliases that canonicalized-path comparison cannot see.
///
/// Limitation: filesystems that cannot supply a meaningful index still yield a
/// tuple; callers decide whether unknown identity must fail closed.
///
/// Returns `Ok(None)` only when `path` does not exist; other failures are
/// reported as errors.
#[cfg(windows)]
pub fn windows_file_identity(path: &Path) -> Result<Option<(u32, u32, u32)>> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use winapi::um::fileapi::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, GetFileInformationByHandle, OPEN_EXISTING,
    };
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::winnt::{
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let display_path = path.display().to_string();
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).wrap_err_with(|| format!("reading file identity {display_path}"));
    }

    // SAFETY: `handle` is valid until CloseHandle below and the zeroed
    // information struct is a plain FFI out-parameter for the duration of
    // this single call.
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let result = unsafe { GetFileInformationByHandle(handle, &mut information) };
    let close_result = unsafe { CloseHandle(handle) };
    if result == 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err_with(|| format!("reading file identity {display_path}"));
    }
    if close_result == 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err_with(|| format!("closing file identity handle {display_path}"));
    }

    Ok(Some((
        information.dwVolumeSerialNumber,
        information.nFileIndexHigh,
        information.nFileIndexLow,
    )))
}
