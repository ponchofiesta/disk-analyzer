# Disk Analyzer

Disk Analyzer is a Rust desktop application for inspecting disk usage with a responsive tree view, live scan progress, and safe file actions. It targets Windows and Linux, using GPUI for the desktop shell and a Windows-specific NTFS/MFT fast path when raw volume access is available.

## Current capabilities

- Live scanning with incremental updates and progress summaries.
- Hierarchical tree view with size aggregation.
- Sorting by size or name.
- Context menu and keyboard-driven navigation.
- Reveal selected files or folders in the native file manager.
- Move selected files or folders to the OS trash or recycle bin.
- Root rescans and subtree rescans.
- Theme switching between system, light, and dark.
- Windows NTFS scanning through MFT traversal, with automatic fallback to standard directory walking when the raw NTFS path cannot be used.

## Platform notes

### Windows

- The scanner tries to enumerate directories through NTFS metadata first for better performance.
- If the selected path is not suitable for raw NTFS access, the app falls back to normal filesystem traversal.
- Revealing files uses Explorer.
- Delete operations move items to the Recycle Bin.

### Linux

- File reveal uses `xdg-open`.
- Delete operations move items to the desktop trash through the `trash` crate.

## Build and run

### Debug build

```powershell
cargo run
```

### Release build

```powershell
cargo build --release
```

## Windows build requirement: `fxc.exe`

GPUI's Windows build currently requires the DirectX shader compiler `fxc.exe` from the Windows SDK.

If `cargo build --release` fails with an error similar to `Failed to find fxc.exe`, either:

- add the appropriate Windows SDK `bin/.../x64` directory to `PATH`, or
- set the `GPUI_FXC_PATH` environment variable to the full `fxc.exe` path.

This repository currently includes a local `.cargo/config.toml` that points to one specific SDK installation:

```toml
[env]
GPUI_FXC_PATH = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\fxc.exe'
```

That path is machine-specific. If your SDK is installed under a different version folder, update it accordingly.

## Project structure

```text
src/
  app.rs                 App bootstrap and scan event polling
  app/
    actions.rs           UI actions, keyboard handling, context menu behavior
    theme.rs             Theme selection and palette definitions
    views.rs             Main window layout and rendering helpers
  model.rs               Tree model, selection state, progress state
  platform.rs            Reveal and trash integration
  scanner.rs             Scan worker, batching, progress throttling
  scanner/
    windows_ntfs.rs      Windows NTFS/MFT traversal
  ui.rs                  Formatting helpers
  lib.rs                 Crate root
  main.rs                Binary entry point
```

## Scanner behavior

Scanning runs on a worker thread and sends batched updates back to the UI. The current implementation is tuned to avoid flooding the main thread with every discovered item immediately:

- entries are grouped into large batches,
- summary progress updates are throttled to roughly every 200 ms,
- UI refreshes are only triggered when new scan events arrive.

On Windows, the NTFS backend walks directory indexes and uses file record numbers to avoid unnecessary reopen work where possible.

## Interaction model

- `Arrow Up` / `Arrow Down`: move selection
- `Arrow Left` / `Arrow Right`: collapse or expand folders
- `Enter`: expand a directory, or reveal a file
- `Space`: toggle the selected directory
- `Shift+F10` or `Context Menu`: open the context menu
- `F5`: rescan the current root
- `R`: rescan the selected subtree
- `Delete`: move the selected item to trash
- `T`: cycle theme preference

## Status

The application is functional and already supports large-tree inspection, safe file actions, and Windows-specific NTFS acceleration. The main remaining performance work is likely in model ingestion and tree rendering rather than raw directory enumeration alone.
