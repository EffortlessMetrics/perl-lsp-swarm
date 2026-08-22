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
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    use winapi::um::winnt::HANDLE;

    let display_path = path.display().to_string();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("opening file identity {display_path}"));
        }
    };

    // SAFETY: `file` owns a valid Windows handle for the duration of this
    // call, and the zeroed information struct is a plain FFI out-parameter.
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let handle = file.as_raw_handle() as HANDLE;
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err_with(|| format!("reading file identity {display_path}"));
    }

    Ok(Some((
        information.dwVolumeSerialNumber,
        information.nFileIndexHigh,
        information.nFileIndexLow,
    )))
}

#[cfg(all(test, windows))]
mod tests {
    use super::windows_file_identity;
    use color_eyre::eyre::{Result, bail};
    use std::fs;

    #[test]
    fn missing_file_identity_is_none() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let missing = directory.path().join("missing.pl");

        if windows_file_identity(&missing)?.is_some() {
            bail!("missing file unexpectedly had a Windows identity: {}", missing.display());
        }

        Ok(())
    }

    #[test]
    fn identity_uses_std_extended_length_path_handling() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut nested = directory.path().to_path_buf();
        while nested.to_string_lossy().chars().count() <= 260 {
            nested.push("long-path-segment-012345678901234567890123456789");
        }
        fs::create_dir_all(&nested)?;
        let file = nested.join("fixture.pl");
        fs::write(&file, b"1;\n")?;

        let path_length = file.to_string_lossy().chars().count();
        if path_length <= 260 {
            bail!("test path did not exceed MAX_PATH: {path_length}");
        }
        if windows_file_identity(&file)?.is_none() {
            bail!("existing extended-length path had no Windows identity: {}", file.display());
        }

        Ok(())
    }
}
