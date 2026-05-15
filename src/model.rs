use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::scanner::{DiscoveredEntry, EntryKind, ScanBatch, ScanEvent, ScanRequest, ScanSummary};

pub type NodeId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    SizeDesc,
    SizeAsc,
    NameAsc,
    NameDesc,
    FilesDesc,
    FilesAsc,
    ModifiedDesc,
    ModifiedAsc,
}

impl SortMode {
    pub fn for_name(descending: bool) -> Self {
        if descending {
            Self::NameDesc
        } else {
            Self::NameAsc
        }
    }

    pub fn for_size(descending: bool) -> Self {
        if descending {
            Self::SizeDesc
        } else {
            Self::SizeAsc
        }
    }

    pub fn for_files(descending: bool) -> Self {
        if descending {
            Self::FilesDesc
        } else {
            Self::FilesAsc
        }
    }

    pub fn for_modified(descending: bool) -> Self {
        if descending {
            Self::ModifiedDesc
        } else {
            Self::ModifiedAsc
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
    pub file_count: u64,
    pub modified_at: Option<SystemTime>,
    pub children: Vec<NodeId>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct VisibleNode {
    pub id: NodeId,
    pub depth: usize,
    pub name: String,
    pub kind: NodeKind,
    pub recursive_size: u64,
    pub file_count: u64,
    pub modified_at: Option<SystemTime>,
    pub expanded: bool,
    pub has_children: bool,
    pub has_error: bool,
    pub is_scanning: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub current_path: Option<PathBuf>,
    pub finished: bool,
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
            .map(|node| node.path.as_path())
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_node().map(|node| node.path.as_path())
    }

    pub fn selected_node(&self) -> Option<&TreeNode> {
        self.selected.filter(|id| self.is_active_node(*id))?;
        self.selected.and_then(|id| self.nodes.get(id))
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

    pub fn apply_event(&mut self, event: ScanEvent) {
        match event {
            ScanEvent::Started { request } => self.begin_scan(request),
            ScanEvent::Batch(batch) => self.apply_batch(batch),
            ScanEvent::Finished { request, summary } => self.finish_scan(request, summary),
            ScanEvent::Cancelled { request } => self.cancel_scan(request),
        }
    }

    pub fn set_sort_mode(&mut self, sort_mode: SortMode) {
        self.sort_mode = sort_mode;
    }

    pub fn toggle_expanded(&mut self, id: NodeId) {
        if !self.is_active_node(id) {
            return;
        }

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
        if self.is_active_node(id) {
            self.selected = Some(id);
        }
    }

    pub fn set_context_target(&mut self, id: Option<NodeId>) {
        self.context_target = id;
        if let Some(id) = id {
            self.select(id);
        }
    }

    pub fn mark_deleted(&mut self, id: NodeId) {
        if !self.is_active_node(id) {
            return;
        }

        let Some(node) = self.nodes.get(id) else {
            return;
        };

        if Some(id) == self.root {
            self.clear_all();
            return;
        }

        let parent_id = node.parent;
        let removed_size = node.recursive_size;
        let removed_file_count = node.file_count;

        self.mark_removed(id);

        if let Some(parent_id) = parent_id {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                parent.children.retain(|child_id| *child_id != id);
            }
            self.propagate_size_delta(Some(parent_id), -(removed_size as i128));
            self.propagate_file_count_delta(Some(parent_id), -(removed_file_count as i128));
            self.selected = Some(parent_id);
        } else {
            self.selected = None;
        }

        self.compact_nodes();
    }

    pub fn visible_nodes(&self) -> Vec<VisibleNode> {
        let mut rows = Vec::new();
        if let Some(root) = self.root {
            self.push_visible(root, 0, &mut rows);
        }
        rows
    }

    fn is_node_in_active_scan_chain(&self, node: &TreeNode) -> bool {
        self.scan_state
            .as_ref()
            .filter(|state| !state.progress.finished)
            .and_then(|state| state.progress.current_path.as_ref())
            .is_some_and(|current_path| current_path.starts_with(&node.path))
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
        let root_modified_at = fs::metadata(&request.target)
            .ok()
            .and_then(|metadata| metadata.modified().ok());

        if request.is_root_scan() {
            self.clear_all();
            let root_id = self.insert_node(
                None,
                request.target.clone(),
                NodeKind::Directory,
                root_modified_at,
                None,
            );
            self.root = Some(root_id);
            self.selected = Some(root_id);
            self.expanded.insert(root_id);
        } else if self.path_index.contains_key(&request.target) {
            if let Some(target_id) = self.path_index.get(&request.target).copied() {
                self.nodes[target_id].modified_at = root_modified_at;
            }
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
        let removed_file_count = self.nodes[target_id].file_count;
        let children = self.nodes[target_id].children.clone();
        for child_id in children {
            self.mark_removed(child_id);
        }

        self.nodes[target_id].children.clear();
        self.nodes[target_id].recursive_size = 0;
        self.nodes[target_id].file_count = 0;
        self.nodes[target_id].last_error = None;

        if let Some(parent_id) = self.nodes[target_id].parent {
            self.propagate_size_delta(Some(parent_id), -(removed_size as i128));
            self.propagate_file_count_delta(Some(parent_id), -(removed_file_count as i128));
        }

        self.compact_nodes();
    }

    fn mark_removed(&mut self, node_id: NodeId) {
        if !self.is_active_node(node_id) {
            return;
        }

        let children = self.nodes[node_id].children.clone();
        let path = self.nodes[node_id].path.clone();
        let parent = self.nodes[node_id].parent;
        self.path_index.remove(&path);
        if self.context_target == Some(node_id) {
            self.context_target = None;
        }
        if self.selected == Some(node_id) {
            self.selected = parent;
        }

        for child in children {
            self.mark_removed(child);
        }
    }

    fn is_active_node(&self, node_id: NodeId) -> bool {
        let Some(node) = self.nodes.get(node_id) else {
            return false;
        };

        self.path_index.get(&node.path).copied() == Some(node_id)
    }

    fn compact_nodes(&mut self) {
        let Some(root_id) = self.root else {
            self.nodes.clear();
            self.path_index.clear();
            self.expanded.clear();
            self.selected = None;
            self.context_target = None;
            return;
        };

        if !self.is_active_node(root_id) {
            self.clear_all();
            return;
        }

        let mut ordered_ids = Vec::new();
        let mut old_to_new = vec![None; self.nodes.len()];
        self.collect_reachable_nodes(root_id, &mut old_to_new, &mut ordered_ids);

        let mut new_nodes = Vec::with_capacity(ordered_ids.len());
        let mut new_path_index = HashMap::with_capacity(self.path_index.len());

        for old_id in ordered_ids {
            let mut node = self.nodes[old_id].clone();
            let new_id = old_to_new[old_id].expect("reachable node id should be remapped");
            node.id = new_id;
            node.parent = node.parent.and_then(|parent_id| old_to_new[parent_id]);
            node.children = node
                .children
                .into_iter()
                .filter_map(|child_id| old_to_new[child_id])
                .collect();
            new_path_index.insert(node.path.clone(), new_id);
            new_nodes.push(node);
        }

        self.nodes = new_nodes;
        self.path_index = new_path_index;
        self.root = old_to_new[root_id];
        self.expanded = self
            .expanded
            .iter()
            .filter_map(|id| old_to_new.get(*id).and_then(|mapped| *mapped))
            .collect();
        self.selected = self
            .selected
            .and_then(|id| old_to_new.get(id).and_then(|mapped| *mapped));
        self.context_target = self
            .context_target
            .and_then(|id| old_to_new.get(id).and_then(|mapped| *mapped));
    }

    fn collect_reachable_nodes(
        &self,
        node_id: NodeId,
        old_to_new: &mut [Option<NodeId>],
        ordered_ids: &mut Vec<NodeId>,
    ) {
        if old_to_new[node_id].is_some() {
            return;
        }

        old_to_new[node_id] = Some(ordered_ids.len());
        ordered_ids.push(node_id);

        for &child_id in &self.nodes[node_id].children {
            self.collect_reachable_nodes(child_id, old_to_new, ordered_ids);
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
                entry.modified_at,
                entry.error.clone(),
            )
        };

        if let Some(node) = self.nodes.get_mut(node_id) {
            node.kind = node_kind;
            node.modified_at = entry.modified_at;
            node.last_error = entry.error;
        }

        if node_kind == NodeKind::File {
            let old_size = self.nodes[node_id].recursive_size;
            let old_file_count = self.nodes[node_id].file_count;
            let new_size = entry.size;
            let delta = new_size as i128 - old_size as i128;
            self.nodes[node_id].recursive_size = new_size;
            self.nodes[node_id].file_count = 1;
            self.propagate_size_delta(self.nodes[node_id].parent, delta);
            self.propagate_file_count_delta(
                self.nodes[node_id].parent,
                self.nodes[node_id].file_count as i128 - old_file_count as i128,
            );
        }
    }

    fn insert_node(
        &mut self,
        parent_id: Option<NodeId>,
        path: PathBuf,
        kind: NodeKind,
        modified_at: Option<SystemTime>,
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
            file_count: 0,
            modified_at,
            children: Vec::new(),
            last_error: error,
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

    fn propagate_file_count_delta(&mut self, mut current: Option<NodeId>, delta: i128) {
        if delta == 0 {
            return;
        }

        while let Some(node_id) = current {
            let node = &mut self.nodes[node_id];
            if delta.is_negative() {
                node.file_count = node.file_count.saturating_sub(delta.unsigned_abs() as u64);
            } else {
                node.file_count = node.file_count.saturating_add(delta as u64);
            }
            current = node.parent;
        }
    }

    fn push_visible(&self, node_id: NodeId, depth: usize, rows: &mut Vec<VisibleNode>) {
        let Some(node) = self.nodes.get(node_id) else {
            return;
        };

        rows.push(VisibleNode {
            id: node.id,
            depth,
            name: node.name.clone(),
            kind: node.kind,
            recursive_size: node.recursive_size,
            file_count: node.file_count,
            modified_at: node.modified_at,
            expanded: self.expanded.contains(&node.id),
            has_children: !node.children.is_empty(),
            has_error: node.last_error.is_some(),
            is_scanning: self.is_node_in_active_scan_chain(node),
        });

        if !self.expanded.contains(&node.id) {
            return;
        }

        let mut children = node.children.clone();
        children.sort_by(|left, right| {
            let left = &self.nodes[*left];
            let right = &self.nodes[*right];
            match self.sort_mode {
                SortMode::SizeDesc => right
                    .recursive_size
                    .cmp(&left.recursive_size)
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::SizeAsc => left
                    .recursive_size
                    .cmp(&right.recursive_size)
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::NameAsc => left
                    .name
                    .to_lowercase()
                    .cmp(&right.name.to_lowercase())
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size)),
                SortMode::NameDesc => right
                    .name
                    .to_lowercase()
                    .cmp(&left.name.to_lowercase())
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size)),
                SortMode::FilesDesc => right
                    .file_count
                    .cmp(&left.file_count)
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size))
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::FilesAsc => left
                    .file_count
                    .cmp(&right.file_count)
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size))
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::ModifiedDesc => right
                    .modified_at
                    .cmp(&left.modified_at)
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size))
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
                SortMode::ModifiedAsc => left
                    .modified_at
                    .cmp(&right.modified_at)
                    .then_with(|| right.recursive_size.cmp(&left.recursive_size))
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())),
            }
        });

        for child_id in children {
            self.push_visible(child_id, depth + 1, rows);
        }
    }
}
