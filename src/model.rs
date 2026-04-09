use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::scanner::{DiscoveredEntry, EntryKind, ScanBatch, ScanEvent, ScanRequest, ScanSummary};

pub type NodeId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    SizeDesc,
    NameAsc,
}

impl SortMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::SizeDesc => Self::NameAsc,
            Self::NameAsc => Self::SizeDesc,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SizeDesc => "Sort: Size",
            Self::NameAsc => "Sort: Name",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl NodeKind {
    pub fn from_entry_kind(kind: EntryKind) -> Self {
        match kind {
            EntryKind::Directory => Self::Directory,
            EntryKind::File => Self::File,
            EntryKind::Symlink => Self::Symlink,
            EntryKind::Other => Self::Other,
        }
    }

    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub path: PathBuf,
    pub name: String,
    pub kind: NodeKind,
    pub depth: usize,
    pub recursive_size: u64,
    pub children: Vec<NodeId>,
    pub last_error: Option<String>,
    pub removed: bool,
}

#[derive(Clone, Debug)]
pub struct VisibleNode {
    pub id: NodeId,
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
    pub recursive_size: u64,
    pub selected: bool,
    pub expanded: bool,
    pub has_children: bool,
    pub has_error: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub current_path: Option<PathBuf>,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub bytes_scanned: u64,
    pub directories_discovered: u64,
    pub finished: bool,
}

impl ProgressSnapshot {
    pub fn fraction(&self) -> f32 {
        if self.finished {
            return 1.0;
        }

        let denominator = self
            .directories_discovered
            .max(self.directories_scanned)
            .max(1);
        (self.directories_scanned as f32 / denominator as f32).clamp(0.02, 0.98)
    }
}

#[derive(Clone, Debug)]
pub struct ScanState {
    pub request: ScanRequest,
    pub started_at: Instant,
    pub progress: ProgressSnapshot,
    pub summary: Option<ScanSummary>,
}

#[derive(Clone, Debug)]
pub struct AppModel {
    pub root: Option<NodeId>,
    pub nodes: Vec<TreeNode>,
    pub path_index: HashMap<PathBuf, NodeId>,
    pub expanded: BTreeSet<NodeId>,
    pub selected: Option<NodeId>,
    pub context_target: Option<NodeId>,
    pub sort_mode: SortMode,
    pub scan_state: Option<ScanState>,
    pub status_message: String,
    pub warnings: Vec<String>,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            root: None,
            nodes: Vec::new(),
            path_index: HashMap::new(),
            expanded: BTreeSet::new(),
            selected: None,
            context_target: None,
            sort_mode: SortMode::SizeDesc,
            scan_state: None,
            status_message: String::from("Select a directory to begin scanning."),
            warnings: Vec::new(),
        }
    }
}

impl AppModel {
    pub fn active_root_path(&self) -> Option<&Path> {
        self.root
            .and_then(|root| self.nodes.get(root))
            .filter(|node| !node.removed)
            .map(|node| node.path.as_path())
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_node().map(|node| node.path.as_path())
    }

    pub fn selected_node(&self) -> Option<&TreeNode> {
        self.selected
            .and_then(|id| self.nodes.get(id))
            .filter(|node| !node.removed)
    }

    pub fn last_scan_duration(&self) -> Option<Duration> {
        self.scan_state.as_ref().map(|state| {
            state
                .summary
                .as_ref()
                .map(|summary| summary.elapsed)
                .unwrap_or_else(|| state.started_at.elapsed())
        })
    }

    pub fn progress(&self) -> ProgressSnapshot {
        self.scan_state
            .as_ref()
            .map(|state| state.progress.clone())
            .unwrap_or_default()
    }

    pub fn apply_event(&mut self, event: ScanEvent) {
        match event {
            ScanEvent::Started { request } => self.begin_scan(request),
            ScanEvent::Batch(batch) => self.apply_batch(batch),
            ScanEvent::Finished { request, summary } => self.finish_scan(request, summary),
            ScanEvent::Cancelled { request } => self.cancel_scan(request),
        }
    }

    pub fn toggle_sort_mode(&mut self) {
        self.sort_mode = self.sort_mode.toggle();
    }

    pub fn toggle_expanded(&mut self, id: NodeId) {
        if !self
            .nodes
            .get(id)
            .is_some_and(|node| node.kind.is_directory())
        {
            return;
        }

        if !self.expanded.insert(id) {
            self.expanded.remove(&id);
        }
    }

    pub fn select(&mut self, id: NodeId) {
        if self.nodes.get(id).is_some_and(|node| !node.removed) {
            self.selected = Some(id);
        }
    }

    pub fn set_context_target(&mut self, id: Option<NodeId>) {
        self.context_target = id;
        if let Some(id) = id {
            self.select(id);
        }
    }

    pub fn visible_nodes(&self) -> Vec<VisibleNode> {
        let mut rows = Vec::new();
        if let Some(root) = self.root {
            self.push_visible(root, 0, &mut rows);
        }
        rows
    }

    pub fn move_selection(&mut self, delta: isize) {
        let rows = self.visible_nodes();
        if rows.is_empty() {
            return;
        }

        let current_index = self
            .selected
            .and_then(|selected| rows.iter().position(|row| row.id == selected))
            .unwrap_or(0);
        let max_index = rows.len().saturating_sub(1) as isize;
        let next_index = (current_index as isize + delta).clamp(0, max_index) as usize;
        self.selected = Some(rows[next_index].id);
    }

    pub fn collapse_or_select_parent(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };

        if self.expanded.remove(&selected) {
            return;
        }

        if let Some(parent) = self.nodes.get(selected).and_then(|node| node.parent) {
            self.selected = Some(parent);
        }
    }

    pub fn expand_selected(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };

        if self
            .nodes
            .get(selected)
            .is_some_and(|node| node.kind.is_directory())
        {
            self.expanded.insert(selected);
        }
    }

    fn begin_scan(&mut self, request: ScanRequest) {
        self.status_message = format!("Scanning {}", request.target.display());
        self.warnings.clear();

        if request.is_root_scan() {
            self.clear_all();
            let root_id = self.insert_node(None, request.target.clone(), NodeKind::Directory, None);
            self.root = Some(root_id);
            self.selected = Some(root_id);
            self.expanded.insert(root_id);
        } else if self.path_index.contains_key(&request.target) {
            self.reset_subtree_for_rescan(&request.target);
        }

        self.scan_state = Some(ScanState {
            request,
            started_at: Instant::now(),
            progress: ProgressSnapshot::default(),
            summary: None,
        });
    }

    fn apply_batch(&mut self, batch: ScanBatch) {
        if self
            .scan_state
            .as_ref()
            .map(|state| state.request.session_id)
            != Some(batch.session_id)
        {
            return;
        }

        for entry in batch.entries {
            self.upsert_entry(entry);
        }

        if let Some(state) = self.scan_state.as_mut() {
            state.progress = batch.progress;
        }

        for warning in batch.warnings {
            self.warnings.push(warning);
        }

        if let Some(last_warning) = self.warnings.last() {
            self.status_message = last_warning.clone();
        }
    }

    fn finish_scan(&mut self, request: ScanRequest, summary: ScanSummary) {
        if self
            .scan_state
            .as_ref()
            .map(|state| state.request.session_id)
            != Some(request.session_id)
        {
            return;
        }

        if let Some(state) = self.scan_state.as_mut() {
            state.progress.finished = true;
            state.progress.current_path = Some(request.target.clone());
            state.summary = Some(summary.clone());
        }

        self.status_message = format!(
            "Scan complete in {:.1}s across {} files and {} folders.",
            summary.elapsed.as_secs_f32(),
            summary.files_scanned,
            summary.directories_scanned
        );
    }

    fn cancel_scan(&mut self, request: ScanRequest) {
        if self
            .scan_state
            .as_ref()
            .map(|state| state.request.session_id)
            != Some(request.session_id)
        {
            return;
        }

        self.status_message = format!("Cancelled scan for {}", request.target.display());
        self.scan_state = None;
    }

    fn clear_all(&mut self) {
        self.root = None;
        self.nodes.clear();
        self.path_index.clear();
        self.expanded.clear();
        self.selected = None;
        self.context_target = None;
    }

    fn reset_subtree_for_rescan(&mut self, target: &Path) {
        let Some(target_id) = self.path_index.get(target).copied() else {
            return;
        };

        if Some(target_id) == self.root {
            self.clear_all();
            return;
        }

        let removed_size = self.nodes[target_id].recursive_size;
        let children = self.nodes[target_id].children.clone();
        for child_id in children {
            self.mark_removed(child_id);
        }

        self.nodes[target_id].children.clear();
        self.nodes[target_id].recursive_size = 0;
        self.nodes[target_id].last_error = None;

        if let Some(parent_id) = self.nodes[target_id].parent {
            self.propagate_size_delta(Some(parent_id), -(removed_size as i128));
        }
    }

    fn mark_removed(&mut self, node_id: NodeId) {
        if self.nodes.get(node_id).is_none() || self.nodes[node_id].removed {
            return;
        }

        let children = self.nodes[node_id].children.clone();
        let path = self.nodes[node_id].path.clone();
        self.nodes[node_id].removed = true;
        self.path_index.remove(&path);
        if self.context_target == Some(node_id) {
            self.context_target = None;
        }
        if self.selected == Some(node_id) {
            self.selected = None;
        }

        for child in children {
            self.mark_removed(child);
        }
    }

    fn upsert_entry(&mut self, entry: DiscoveredEntry) {
        let node_kind = NodeKind::from_entry_kind(entry.kind);
        let parent_id = entry
            .parent_path
            .as_ref()
            .and_then(|path| self.path_index.get(path))
            .copied();

        let node_id = if let Some(existing_id) = self.path_index.get(&entry.path).copied() {
            existing_id
        } else {
            self.insert_node(
                parent_id,
                entry.path.clone(),
                node_kind,
                entry.error.clone(),
            )
        };

        if let Some(node) = self.nodes.get_mut(node_id) {
            node.kind = node_kind;
            node.last_error = entry.error;
        }

        if node_kind == NodeKind::File {
            let old_size = self.nodes[node_id].recursive_size;
            let new_size = entry.size;
            let delta = new_size as i128 - old_size as i128;
            self.nodes[node_id].recursive_size = new_size;
            self.propagate_size_delta(self.nodes[node_id].parent, delta);
        }
    }

    fn insert_node(
        &mut self,
        parent_id: Option<NodeId>,
        path: PathBuf,
        kind: NodeKind,
        error: Option<String>,
    ) -> NodeId {
        let depth = parent_id.map(|id| self.nodes[id].depth + 1).unwrap_or(0);
        let id = self.nodes.len();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| path.display().to_string());
        self.nodes.push(TreeNode {
            id,
            parent: parent_id,
            path: path.clone(),
            name,
            kind,
            depth,
            recursive_size: 0,
            children: Vec::new(),
            last_error: error,
            removed: false,
        });

        self.path_index.insert(path, id);

        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        id
    }

    fn propagate_size_delta(&mut self, mut current: Option<NodeId>, delta: i128) {
        if delta == 0 {
            return;
        }

        while let Some(node_id) = current {
            let node = &mut self.nodes[node_id];
            if delta.is_negative() {
                node.recursive_size = node
                    .recursive_size
                    .saturating_sub(delta.unsigned_abs() as u64);
            } else {
                node.recursive_size = node.recursive_size.saturating_add(delta as u64);
            }
            current = node.parent;
        }
    }

    fn push_visible(&self, node_id: NodeId, depth: usize, rows: &mut Vec<VisibleNode>) {
        let Some(node) = self.nodes.get(node_id) else {
            return;
        };
        if node.removed {
            return;
        }

        rows.push(VisibleNode {
            id: node.id,
            depth,
            name: node.name.clone(),
            path: node.path.clone(),
            kind: node.kind,
            recursive_size: node.recursive_size,
            selected: self.selected == Some(node.id),
            expanded: self.expanded.contains(&node.id),
            has_children: node
                .children
                .iter()
                .any(|child| !self.nodes[*child].removed),
            has_error: node.last_error.is_some(),
        });

        if !self.expanded.contains(&node.id) {
            return;
        }

        let mut children = node.children.clone();
        children.retain(|child| {
            self.nodes
                .get(*child)
                .is_some_and(|child_node| !child_node.removed)
        });
        children.sort_by(|left, right| {
            let left = &self.nodes[*left];
            let right = &self.nodes[*right];
            match self.sort_mode {
                SortMode::SizeDesc => right
                    .recursive_size
                    .cmp(&left.recursive_size)
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::NameAsc => left
                    .name
                    .to_lowercase()
                    .cmp(&right.name.to_lowercase())
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size)),
            }
        });

        for child_id in children {
            self.push_visible(child_id, depth + 1, rows);
        }
    }
}
