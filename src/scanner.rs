use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{unbounded, Receiver, Sender};

#[cfg(windows)]
mod windows_find;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub(super) const ENTRY_BATCH_SIZE: usize = 32_768;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
pub struct DiscoveredEntry {
    pub path: PathBuf,
    pub parent_path: Option<PathBuf>,
    pub kind: EntryKind,
    pub size: u64,
    pub modified_at: Option<SystemTime>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressInfo {
    pub current_path: Option<PathBuf>,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub bytes_scanned: u64,
    pub directories_discovered: u64,
    pub finished: bool,
}

#[derive(Clone, Debug)]
pub struct ScanBatch {
    pub session_id: u64,
    pub entries: Vec<DiscoveredEntry>,
    pub progress: crate::model::ProgressSnapshot,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ScanSummary {
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct ScanRequest {
    pub session_id: u64,
    pub target: PathBuf,
    pub mode: ScanMode,
}

impl ScanRequest {
    pub fn root(target: PathBuf) -> Self {
        let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            session_id,
            target,
            mode: ScanMode::Root,
        }
    }

    pub fn subtree(target: PathBuf) -> Self {
        let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            session_id,
            target,
            mode: ScanMode::Subtree,
        }
    }

    pub fn is_root_scan(&self) -> bool {
        matches!(self.mode, ScanMode::Root)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanMode {
    Root,
    Subtree,
}

#[derive(Clone, Debug)]
pub enum ScanEvent {
    Started {
        request: ScanRequest,
    },
    Batch(ScanBatch),
    Finished {
        request: ScanRequest,
        summary: ScanSummary,
    },
    Cancelled {
        request: ScanRequest,
    },
}

#[derive(Debug)]
pub struct ScanHandle {
    pub receiver: Receiver<ScanEvent>,
    cancelled: Arc<AtomicBool>,
}

impl ScanHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub fn spawn_scan(request: ScanRequest) -> ScanHandle {
    let (sender, receiver) = unbounded();
    let cancelled = Arc::new(AtomicBool::new(false));
    let request_clone = request.clone();
    let cancel_flag = Arc::clone(&cancelled);

    thread::Builder::new()
        .name(format!("scan-{}", request.session_id))
        .spawn(move || run_scan(request_clone, sender, cancel_flag))
        .expect("failed to spawn scan thread");

    ScanHandle {
        receiver,
        cancelled,
    }
}

fn run_scan(request: ScanRequest, sender: Sender<ScanEvent>, cancelled: Arc<AtomicBool>) {
    #[cfg(windows)]
    if windows_find::try_run_scan(&request, &sender, &cancelled) {
        return;
    }

    run_standard_scan(request, sender, cancelled);
}

fn run_standard_scan(request: ScanRequest, sender: Sender<ScanEvent>, cancelled: Arc<AtomicBool>) {
    let _ = sender.send(ScanEvent::Started {
        request: request.clone(),
    });

    let start = Instant::now();
    let mut progress = ProgressInfo {
        current_path: Some(request.target.clone()),
        directories_discovered: 1,
        ..Default::default()
    };
    let mut pending_entries = Vec::with_capacity(ENTRY_BATCH_SIZE);
    let mut warnings = Vec::new();
    let mut last_flush = Instant::now();

    let mut stack = vec![request.target.clone()];
    while let Some(directory) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(ScanEvent::Cancelled {
                request: request.clone(),
            });
            return;
        }

        progress.current_path = Some(directory.clone());
        progress.directories_scanned += 1;

        let children = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!("{}: {}", directory.display(), error));
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

        for child in children {
            if cancelled.load(Ordering::Relaxed) {
                let _ = sender.send(ScanEvent::Cancelled {
                    request: request.clone(),
                });
                return;
            }

            let child = match child {
                Ok(child) => child,
                Err(error) => {
                    warnings.push(error.to_string());
                    continue;
                }
            };

            let path = child.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    warnings.push(format!("{}: {}", path.display(), error));
                    pending_entries.push(DiscoveredEntry {
                        path: path.clone(),
                        parent_path: Some(directory.clone()),
                        kind: EntryKind::Other,
                        size: 0,
                        modified_at: None,
                        error: Some(error.to_string()),
                    });
                    maybe_flush(
                        &sender,
                        &request,
                        &mut pending_entries,
                        &progress,
                        &mut warnings,
                        &mut last_flush,
                    );
                    continue;
                }
            };

            let file_type = metadata.file_type();
            let modified_at = metadata.modified().ok();
            if file_type.is_dir() && !file_type.is_symlink() {
                progress.directories_discovered += 1;
                stack.push(path.clone());
                pending_entries.push(DiscoveredEntry {
                    path,
                    parent_path: Some(directory.clone()),
                    kind: EntryKind::Directory,
                    size: 0,
                    modified_at,
                    error: None,
                });
            } else if file_type.is_file() {
                progress.files_scanned += 1;
                progress.bytes_scanned += metadata.len();
                pending_entries.push(DiscoveredEntry {
                    path,
                    parent_path: Some(directory.clone()),
                    kind: EntryKind::File,
                    size: metadata.len(),
                    modified_at,
                    error: None,
                });
            } else if file_type.is_symlink() {
                pending_entries.push(DiscoveredEntry {
                    path,
                    parent_path: Some(directory.clone()),
                    kind: EntryKind::Symlink,
                    size: 0,
                    modified_at,
                    error: None,
                });
            } else {
                pending_entries.push(DiscoveredEntry {
                    path,
                    parent_path: Some(directory.clone()),
                    kind: EntryKind::Other,
                    size: 0,
                    modified_at,
                    error: None,
                });
            }

            maybe_flush(
                &sender,
                &request,
                &mut pending_entries,
                &progress,
                &mut warnings,
                &mut last_flush,
            );
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

fn maybe_flush(
    sender: &Sender<ScanEvent>,
    request: &ScanRequest,
    pending_entries: &mut Vec<DiscoveredEntry>,
    progress: &ProgressInfo,
    warnings: &mut Vec<String>,
    last_flush: &mut Instant,
) {
    if pending_entries.len() >= ENTRY_BATCH_SIZE {
        flush_batch(sender, request, pending_entries, progress, warnings);
        *last_flush = Instant::now();
    } else if last_flush.elapsed() >= PROGRESS_INTERVAL {
        emit_progress_batch(sender, request, progress, warnings);
        *last_flush = Instant::now();
    }
}

fn emit_progress_batch(
    sender: &Sender<ScanEvent>,
    request: &ScanRequest,
    progress: &ProgressInfo,
    warnings: &mut Vec<String>,
) {
    let _ = sender.send(ScanEvent::Batch(ScanBatch {
        session_id: request.session_id,
        entries: Vec::new(),
        progress: crate::model::ProgressSnapshot {
            current_path: progress.current_path.clone(),
            files_scanned: progress.files_scanned,
            directories_scanned: progress.directories_scanned,
            bytes_scanned: progress.bytes_scanned,
            finished: progress.finished,
        },
        warnings: std::mem::take(warnings),
    }));
}

fn flush_batch(
    sender: &Sender<ScanEvent>,
    request: &ScanRequest,
    pending_entries: &mut Vec<DiscoveredEntry>,
    progress: &ProgressInfo,
    warnings: &mut Vec<String>,
) {
    if pending_entries.is_empty() && warnings.is_empty() {
        return;
    }

    let entries = std::mem::take(pending_entries);
    let warning_messages = std::mem::take(warnings);
    let _ = sender.send(ScanEvent::Batch(ScanBatch {
        session_id: request.session_id,
        entries,
        progress: crate::model::ProgressSnapshot {
            current_path: progress.current_path.clone(),
            files_scanned: progress.files_scanned,
            directories_scanned: progress.directories_scanned,
            bytes_scanned: progress.bytes_scanned,
            finished: progress.finished,
        },
        warnings: warning_messages,
    }));
}
