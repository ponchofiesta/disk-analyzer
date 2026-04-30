use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;

#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_WANTNUKEWARNING,
    FO_DELETE, SHFILEOPSTRUCTW,
};

pub fn reveal_in_file_manager(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("explorer");
        if path.is_dir() {
            command.arg(path);
        } else {
            command.arg(format!("/select,{}", path.display()));
        }

        command
            .status()
            .context("failed to launch Windows Explorer")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("Explorer returned a non-success status"))
    }

    #[cfg(target_os = "linux")]
    {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };

        Command::new("xdg-open")
            .arg(target)
            .status()
            .context("failed to launch xdg-open")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("xdg-open returned a non-success status"))
    }
}

pub fn trash_path(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        trash_path_windows(path)
    }

    #[cfg(not(target_os = "windows"))]
    {
        trash::delete(path).with_context(|| format!("failed to move {} to trash", path.display()))
    }
}

#[cfg(target_os = "windows")]
fn trash_path_windows(path: &Path) -> Result<()> {
    let mut from = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();

    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_mut_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOCONFIRMMKDIR | FOF_WANTNUKEWARNING).0
            as u16,
        ..Default::default()
    };

    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(anyhow!(
            "failed to move {} to trash (Windows shell error {result})",
            path.display()
        ));
    }

    if operation.fAnyOperationsAborted.as_bool() {
        return Err(anyhow!("delete operation was cancelled"));
    }

    Ok(())
}
