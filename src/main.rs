use std::{env, ffi::OsString, path::PathBuf};

use anyhow::{bail, Result};

fn main() -> Result<()> {
    let initial_scan_target = parse_initial_scan_target(env::args_os())?;
    disk_analyzer::run(initial_scan_target)
}

fn parse_initial_scan_target<I>(args: I) -> Result<Option<PathBuf>>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();

    let Some(path) = args.next() else {
        return Ok(None);
    };

    if args.next().is_some() {
        bail!("Usage: disk-analyzer [directory]");
    }

    let path = PathBuf::from(path);
    if !path.exists() {
        bail!("Startup scan path does not exist: {}", path.display());
    }

    if !path.is_dir() {
        bail!("Startup scan path must be a directory: {}", path.display());
    }

    Ok(Some(path))
}
