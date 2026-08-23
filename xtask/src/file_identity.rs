//! Kernel-level file identity helpers shared across xtask targets.
//!
//! Hard-link-accurate identity is not available from stable `std` on Windows
//! (`windows_by_handle` metadata APIs remain unstable), so the one primitive
//! here reads it through the already-vendored `winapi` dependency instead.

#[cfg(windows)]
use color_eyre::eyre::{Result, WrapErr};
#[cfg(windows)]
use std::path::Path;

/// The collision-resistant identity returned by Windows `FileIdInfo`.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsFileIdentity {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

/// Read the kernel-level identity of an existing file on Windows.
///
/// The identity contains the volume serial number and the complete 128-bit
/// file identifier returned by `GetFileInformationByHandleEx`. An unsupported
/// filesystem, unavailable API, or incomplete identifier is reported as an
/// error so callers fail closed instead of treating an unknown identity as a
/// safe mismatch.
///
/// Returns `Ok(None)` only when `path` does not exist; other failures are
/// reported as errors.
#[cfg(windows)]
pub fn windows_file_identity(path: &Path) -> Result<Option<WindowsFileIdentity>> {
    use std::mem::size_of;
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use winapi::um::fileapi::FILE_ID_INFO;
    use winapi::um::minwinbase::FileIdInfo;
    use winapi::um::winbase::GetFileInformationByHandleEx;
    use winapi::um::winnt::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, HANDLE};

    let display_path = path.display().to_string();
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .wrap_err_with(|| format!("reading file identity metadata {display_path}"));
        }
    }

    let file = match std::fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
    {
        Ok(file) => file,
        // `symlink_metadata` above proved that the path itself existed. A
        // subsequent NotFound therefore means that the target disappeared or
        // that this is a dangling symlink, so the identity is unknown and the
        // caller must fail closed instead of treating it as an absent source.
        Err(error) => {
            return Err(error).wrap_err_with(|| format!("opening file identity {display_path}"));
        }
    };

    // SAFETY: `file` owns a valid Windows handle for the duration of this
    // call, and the zeroed information struct is a plain FFI out-parameter.
    let mut information: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    let handle = file.as_raw_handle() as HANDLE;
    let result = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            &mut information as *mut FILE_ID_INFO as *mut _,
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err_with(|| format!("reading Windows FileIdInfo identity {display_path}"));
    }

    let file_id = information.FileId.Identifier;
    if information.VolumeSerialNumber == 0 || file_id == [0; 16] {
        return Err(color_eyre::eyre::eyre!(
            "Windows FileIdInfo identity is unsupported or incomplete for {display_path}"
        ));
    }

    Ok(Some(WindowsFileIdentity { volume_serial_number: information.VolumeSerialNumber, file_id }))
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
    fn identity_uses_std_extended_length_path_handling_and_distinguishes_files() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut nested = directory.path().to_path_buf();
        while nested.to_string_lossy().chars().count() <= 260 {
            nested.push("long-path-segment-012345678901234567890123456789");
        }
        fs::create_dir_all(&nested)?;
        let file = nested.join("fixture.pl");
        let alias = nested.join("fixture-alias.pl");
        let distinct = nested.join("distinct.pl");
        fs::write(&file, b"1;\n")?;
        fs::write(&distinct, b"2;\n")?;
        fs::hard_link(&file, &alias)?;

        let path_length = file.to_string_lossy().chars().count();
        if path_length <= 260 {
            bail!("test path did not exceed MAX_PATH: {path_length}");
        }
        let file_identity = windows_file_identity(&file)?
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture had no Windows identity"))?;
        let alias_identity = windows_file_identity(&alias)?
            .ok_or_else(|| color_eyre::eyre::eyre!("hard-link alias had no Windows identity"))?;
        let distinct_identity = windows_file_identity(&distinct)?
            .ok_or_else(|| color_eyre::eyre::eyre!("distinct file had no Windows identity"))?;

        if file_identity != alias_identity {
            bail!("hard-link alias did not preserve Windows file identity");
        }
        if file_identity == distinct_identity {
            bail!("distinct files unexpectedly shared Windows file identity");
        }

        Ok(())
    }
}
