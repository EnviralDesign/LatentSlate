use std::path::{Path, PathBuf};

#[cfg(windows)]
pub fn move_path_to_recycle_bin(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    let resolved = std::fs::canonicalize(path)
        .map_err(|err| format!("Failed to resolve {}: {err}", path.display()))?;
    let shell_path = shell_compatible_path(&resolved);
    let mut from: Vec<u16> = shell_path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(format!(
            "Windows failed to move {} to the Recycle Bin. Shell error code: {result}.",
            shell_path.display()
        ));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(format!(
            "Windows did not move {} to the Recycle Bin.",
            shell_path.display()
        ));
    }

    Ok(())
}

#[cfg(windows)]
fn shell_compatible_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(stripped) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{stripped}"))
    } else if let Some(stripped) = text.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

#[cfg(not(windows))]
pub fn move_path_to_recycle_bin(_path: &Path) -> Result<(), String> {
    Err("Project deletion to the system recycle bin is only supported on Windows.".to_string())
}
