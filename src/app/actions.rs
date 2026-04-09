use std::path::PathBuf;

use gpui::{
    point, px, AsyncApp, ClickEvent, Context, KeyDownEvent, MouseDownEvent, PathPromptOptions,
    Pixels, PromptLevel, SharedString, WeakEntity, Window,
};

use crate::platform::{reveal_in_file_manager, trash_path};
use crate::scanner::ScanRequest;

use super::DiskAnalyzerApp;

impl DiskAnalyzerApp {
    pub(super) fn choose_directory_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from("Select a directory to analyze")),
        });

        cx.spawn(move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                match picker.await {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            let _ = this.update(&mut cx, |this, cx: &mut Context<Self>| {
                                this.start_scan(ScanRequest::root(path));
                                cx.notify();
                            });
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        let _ = this.update(&mut cx, |this, cx: &mut Context<Self>| {
                            this.model.status_message = format!("Folder picker failed: {error}");
                            cx.notify();
                        });
                    }
                    Err(_) => {}
                }
            }
        })
        .detach();
    }

    pub(super) fn choose_directory_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_directory_impl(window, cx);
    }

    pub(super) fn rescan_root_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rescan_root_action(cx);
    }

    pub(super) fn rescan_root_action(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.model.active_root_path().map(PathBuf::from) else {
            self.model.status_message = String::from("No root directory selected yet.");
            cx.notify();
            return;
        };

        self.start_scan(ScanRequest::root(root));
        cx.notify();
    }

    pub(super) fn rescan_selected_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rescan_selected_action(cx);
    }

    pub(super) fn rescan_selected_action(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected_path().map(PathBuf::from) else {
            self.model.status_message = String::from("Select a file or directory first.");
            cx.notify();
            return;
        };

        self.start_scan(ScanRequest::subtree(selected));
        cx.notify();
    }

    pub(super) fn reveal_selected_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reveal_selected_action(cx);
    }

    pub(super) fn reveal_selected_action(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected_path().map(PathBuf::from) else {
            self.model.status_message = String::from("Select a file or directory first.");
            cx.notify();
            return;
        };

        match reveal_in_file_manager(&selected) {
            Ok(()) => self.model.status_message = format!("Revealed {}", selected.display()),
            Err(error) => self.model.status_message = format!("Reveal failed: {error}"),
        }

        cx.notify();
    }

    pub(super) fn delete_selected_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete_action(window, cx);
    }

    pub(super) fn confirm_delete_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_path) = self.model.selected_path().map(PathBuf::from) else {
            self.model.status_message = String::from("Select a file or directory first.");
            cx.notify();
            return;
        };

        self.close_context_menu_state();

        let answer = window.prompt(
            PromptLevel::Warning,
            "Move selected item to trash?",
            Some(&selected_path.display().to_string()),
            &["Move to Trash", "Cancel"],
            cx,
        );

        cx.spawn(move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if answer.await.ok() == Some(0) {
                    let _ = this.update(&mut cx, |this, cx: &mut Context<Self>| {
                        match trash_path(&selected_path) {
                            Ok(()) => {
                                this.model.status_message =
                                    format!("Moved {} to trash", selected_path.display());
                                this.start_scan(ScanRequest::root(
                                    selected_path
                                        .parent()
                                        .unwrap_or(&selected_path)
                                        .to_path_buf(),
                                ));
                            }
                            Err(error) => {
                                this.model.status_message = format!("Delete failed: {error}");
                            }
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn toggle_theme_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_preference = self.theme_preference.cycle();
        cx.notify();
    }

    pub(super) fn toggle_sort_click(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.toggle_sort_mode();
        cx.notify();
    }

    pub(super) fn select_row(
        &mut self,
        node_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.select(node_id);
        self.close_context_menu_state();
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(super) fn open_context_for_row(
        &mut self,
        node_id: usize,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(super::ContextMenuState { node_id, position });
        self.model.set_context_target(Some(node_id));
        self.model.select(node_id);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(super) fn close_context_menu_state(&mut self) {
        self.context_menu = None;
        self.model.set_context_target(None);
    }

    pub(super) fn dismiss_context_menu(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu.is_some() {
            self.close_context_menu_state();
            cx.notify();
        }
    }

    pub(super) fn toggle_row(&mut self, node_id: usize, _: &mut Window, cx: &mut Context<Self>) {
        self.model.toggle_expanded(node_id);
        cx.notify();
    }

    pub(super) fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected else {
            return;
        };

        self.model.toggle_expanded(selected);
        cx.notify();
    }

    pub(super) fn jump_to_edge(&mut self, end: bool, cx: &mut Context<Self>) {
        let rows = self.model.visible_nodes();
        if let Some(row) = if end { rows.last() } else { rows.first() } {
            self.model.select(row.id);
            cx.notify();
        }
    }

    pub(super) fn keyboard_menu_position(&self) -> gpui::Point<Pixels> {
        let row_index = self
            .model
            .selected
            .and_then(|selected| {
                self.model
                    .visible_nodes()
                    .iter()
                    .position(|row| row.id == selected)
            })
            .unwrap_or(0);

        point(px(340.0), px(250.0 + (row_index.min(10) as f32 * 32.0)))
    }

    pub(super) fn open_context_for_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.model.selected else {
            return;
        };

        self.open_context_for_row(selected, self.keyboard_menu_position(), window, cx);
    }

    pub(super) fn invoke_context_reveal(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.reveal_selected_action(cx);
    }

    pub(super) fn invoke_context_rescan_selection(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.rescan_selected_action(cx);
    }

    pub(super) fn invoke_context_rescan_root(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.rescan_root_action(cx);
    }

    pub(super) fn invoke_context_delete(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete_action(window, cx);
    }

    pub(super) fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_ascii_lowercase();

        if self.context_menu.is_some() {
            if matches!(key.as_str(), "escape" | "esc") {
                self.close_context_menu_state();
                cx.notify();
            }
            return;
        }

        match key.as_str() {
            "arrowdown" | "down" => self.model.move_selection(1),
            "arrowup" | "up" => self.model.move_selection(-1),
            "arrowleft" | "left" => self.model.collapse_or_select_parent(),
            "arrowright" | "right" => self.model.expand_selected(),
            "home" => self.jump_to_edge(false, cx),
            "end" => self.jump_to_edge(true, cx),
            "enter" => {
                if self
                    .model
                    .selected_node()
                    .is_some_and(|node| node.kind.is_directory())
                {
                    self.toggle_selected(cx);
                } else {
                    self.reveal_selected_action(cx);
                }
            }
            "space" => self.toggle_selected(cx),
            "f10" if event.keystroke.modifiers.shift => self.open_context_for_selection(window, cx),
            "contextmenu" | "menu" => self.open_context_for_selection(window, cx),
            "f5" => self.rescan_root_action(cx),
            "t" if !event.keystroke.modifiers.control && !event.keystroke.modifiers.platform => {
                self.theme_preference = self.theme_preference.cycle()
            }
            "r" if !event.keystroke.modifiers.control && !event.keystroke.modifiers.platform => {
                self.rescan_selected_action(cx)
            }
            "escape" | "esc" => self.close_context_menu_state(),
            "delete" => self.confirm_delete_action(window, cx),
            _ => return,
        }

        cx.notify();
    }
}
