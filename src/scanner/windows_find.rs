use std::ffi::c_void;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::Sender;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    FindClose, FindFirstFileExW, FindNextFileW, FindExInfoBasic, FindExSearchNameMatch,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
    WIN32_FIND_DATAW,
};

use super::{
    flush_batch, maybe_flush, DiscoveredEntry, EntryKind, ProgressInfo, ScanEvent, ScanRequest,
    ScanSummary, ENTRY_BATCH_SIZE,
};

struct DirectoryState {
    path: PathBuf,
    path_wide: Vec<u16>,
}

struct FindHandle(HANDLE);

impl Drop for FindHandle {
    fn drop(&mut self) {
        let _ = unsafe { FindClose(self.0) };
    }
}

pub(super) fn try_run_scan(
    request: &ScanRequest,
    sender: &Sender<ScanEvent>,
    cancelled: &Arc<AtomicBool>,
) -> bool {
    run_find_scan(request.clone(), sender.clone(), Arc::clone(cancelled));
    true
}

fn run_find_scan(request: ScanRequest, sender: Sender<ScanEvent>, cancelled: Arc<AtomicBool>) {
    let _ = sender.send(ScanEvent::Started {
        request: request.clone(),
    });

    let start = Instant::now();
    let root_path = request.target.clone();
    let root_wide = path_to_wide(&root_path);
    let mut progress = ProgressInfo {
        current_path: Some(root_path.clone()),
        directories_discovered: 1,
        ..Default::default()
    };
    let mut pending_entries = Vec::with_capacity(ENTRY_BATCH_SIZE);
    let mut warnings = Vec::new();
    let mut last_flush = Instant::now();
    let mut stack = vec![DirectoryState {
        path: root_path,
        path_wide: root_wide,
    }];

    while let Some(directory) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(ScanEvent::Cancelled {
                request: request.clone(),
            });
            return;
        }

        progress.current_path = Some(directory.path.clone());
        progress.directories_scanned += 1;

        let search_pattern = build_search_pattern(&directory.path_wide);
        let mut find_data = WIN32_FIND_DATAW::default();
        let search_handle = unsafe {
            FindFirstFileExW(
                PCWSTR(search_pattern.as_ptr()),
                FindExInfoBasic,
                &mut find_data as *mut _ as *mut c_void,
                FindExSearchNameMatch,
                None,
                FIND_FIRST_EX_LARGE_FETCH,
            )
        };

        let search_handle = match search_handle {
            Ok(search_handle) if search_handle != INVALID_HANDLE_VALUE => search_handle,
            _ => {
                record_find_error(&mut warnings, &directory.path, true);
                flush_batch(
                    &sender,
                    &request,
                    &mut pending_entries,
                    &progress,
                    &mut warnings,
                );
                continue;
            }
        };

        let _search_handle = FindHandle(search_handle);

        loop {
            if cancelled.load(Ordering::Relaxed) {
                let _ = sender.send(ScanEvent::Cancelled {
                    request: request.clone(),
                });
                return;
            }

            process_find_data(
                &directory,
                &find_data,
                &mut stack,
                &mut pending_entries,
                &mut progress,
            );

            maybe_flush(
                &sender,
                &request,
                &mut pending_entries,
                &progress,
                &mut warnings,
                &mut last_flush,
            );

            if unsafe { FindNextFileW(search_handle, &mut find_data) }.is_ok() {
                continue;
            }

            let error = unsafe { GetLastError() };
            if error != ERROR_NO_MORE_FILES && error != ERROR_ACCESS_DENIED {
                warnings.push(format!("{}: {}", directory.path.display(), os_error_message(error)));
            }
            break;
        }
    }

    progress.finished = true;
    flush_batch(
        &sender,
        &request,
        &mut pending_entries,
        &progress,
        &mut warnings,
    );
    let _ = sender.send(ScanEvent::Finished {
        request: request.clone(),
        summary: ScanSummary {
            files_scanned: progress.files_scanned,
            directories_scanned: progress.directories_scanned,
            elapsed: start.elapsed(),
        },
    });
}

fn process_find_data(
    directory: &DirectoryState,
    find_data: &WIN32_FIND_DATAW,
    stack: &mut Vec<DirectoryState>,
    pending_entries: &mut Vec<DiscoveredEntry>,
    progress: &mut ProgressInfo,
) {
    let name = file_name_slice(find_data);
    if name.is_empty() || is_dot_entry(name) {
        return;
    }

    let attributes = find_data.dwFileAttributes;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return;
    }

    let child_wide = join_path(&directory.path_wide, name);
    let child_path = pathbuf_from_wide(&child_wide);
    let is_directory = attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;

    if is_directory {
        progress.directories_discovered += 1;
        stack.push(DirectoryState {
            path: child_path.clone(),
            path_wide: child_wide,
        });
        pending_entries.push(DiscoveredEntry {
            path: child_path,
            parent_path: Some(directory.path.clone()),
            kind: EntryKind::Directory,
            size: 0,
            error: None,
        });
        return;
    }

    let size = ((find_data.nFileSizeHigh as u64) << 32) | find_data.nFileSizeLow as u64;
    progress.files_scanned += 1;
    progress.bytes_scanned += size;
    pending_entries.push(DiscoveredEntry {
        path: child_path,
        parent_path: Some(directory.path.clone()),
        kind: EntryKind::File,
        size,
        error: None,
    });
}

fn record_find_error(warnings: &mut Vec<String>, path: &Path, always_report: bool) {
    let error = unsafe { GetLastError() };
    if !always_report && error == ERROR_ACCESS_DENIED {
        return;
    }

    warnings.push(format!("{}: {}", path.display(), os_error_message(error)));
}

fn os_error_message(error: windows::Win32::Foundation::WIN32_ERROR) -> String {
    std::io::Error::from_raw_os_error(error.0 as i32).to_string()
}

fn path_to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().collect()
}

fn build_search_pattern(path: &[u16]) -> Vec<u16> {
    let mut pattern = Vec::with_capacity(path.len() + 3);
    pattern.extend_from_slice(path);
    if !path.last().is_some_and(|ch| *ch == b'\\' as u16 || *ch == b'/' as u16) {
        pattern.push(b'\\' as u16);
    }
    pattern.push(b'*' as u16);
    pattern.push(0);
    pattern
}

fn join_path(parent: &[u16], child_name: &[u16]) -> Vec<u16> {
    let mut joined = Vec::with_capacity(parent.len() + child_name.len() + 1);
    joined.extend_from_slice(parent);
    if !parent.last().is_some_and(|ch| *ch == b'\\' as u16 || *ch == b'/' as u16) {
        joined.push(b'\\' as u16);
    }
    joined.extend_from_slice(child_name);
    joined
}

fn pathbuf_from_wide(path: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(path))
}

fn file_name_slice(find_data: &WIN32_FIND_DATAW) -> &[u16] {
    let len = find_data
        .cFileName
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(find_data.cFileName.len());
    &find_data.cFileName[..len]
}

fn is_dot_entry(name: &[u16]) -> bool {
    matches!(name, [46] | [46, 46])
}