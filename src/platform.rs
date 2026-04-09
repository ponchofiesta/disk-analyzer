use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

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
    trash::delete(path).with_context(|| format!("failed to move {} to trash", path.display()))
}
