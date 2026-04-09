use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::Receiver;
use gpui::{
    anchored, div, point, prelude::*, px, relative, rgb, size, uniform_list, App, Application,
    AsyncApp, Bounds, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, PromptLevel, SharedString, Timer, WeakEntity, Window, WindowBounds,
    WindowOptions,
};

use crate::model::{AppModel, NodeId};
use crate::platform::{reveal_in_file_manager, trash_path};
use crate::scanner::{spawn_scan, ScanEvent, ScanHandle, ScanRequest};
use crate::ui::{format_bytes, format_duration, shorten_path};

pub fn run() -> Result<()> {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(DiskAnalyzerApp::new),
        )
        .expect("failed to open disk analyzer window");
        cx.activate(true);
    });
    Ok(())
}

struct DiskAnalyzerApp {
    model: AppModel,
    active_scan: Option<ScanHandle>,
    receiver: Option<Receiver<ScanEvent>>,
    focus_handle: FocusHandle,
    context_menu: Option<ContextMenuState>,
}

#[derive(Clone, Copy)]
struct ContextMenuState {
    node_id: NodeId,
    position: gpui::Point<Pixels>,
}

impl DiskAnalyzerApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let app = Self {
            model: AppModel::default(),
            active_scan: None,
            receiver: None,
            focus_handle: cx.focus_handle(),
            context_menu: None,
        };
        app.spawn_event_poller(cx);
        app
    }

    fn spawn_event_poller(&self, cx: &mut Context<Self>) {
        cx.spawn(|this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                loop {
                    Timer::after(Duration::from_millis(50)).await;
                    if this
                        .update(&mut cx, |this, cx: &mut Context<Self>| {
                            this.process_scan_events();
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn process_scan_events(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };

        let mut clear_receiver = false;
        for event in receiver.try_iter() {
            if matches!(
                event,
                ScanEvent::Finished { .. } | ScanEvent::Cancelled { .. }
            ) {
                clear_receiver = true;
            }
            self.model.apply_event(event);
        }

        if clear_receiver {
            self.receiver = None;
            self.active_scan = None;
        }
    }

    fn start_scan(&mut self, request: ScanRequest) {
        if let Some(active_scan) = &self.active_scan {
            active_scan.cancel();
        }

        self.close_context_menu_state();
        let scan_handle = spawn_scan(request);
        self.receiver = Some(scan_handle.receiver.clone());
        self.active_scan = Some(scan_handle);
    }

    fn choose_directory(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    fn rescan_root(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.rescan_root_action(cx);
    }

    fn rescan_root_action(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.model.active_root_path().map(PathBuf::from) else {
            self.model.status_message = String::from("No root directory selected yet.");
            cx.notify();
            return;
        };

        self.start_scan(ScanRequest::root(root));
        cx.notify();
    }

    fn rescan_selected(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.rescan_selected_action(cx);
    }

    fn rescan_selected_action(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected_path().map(PathBuf::from) else {
            self.model.status_message = String::from("Select a file or directory first.");
            cx.notify();
            return;
        };

        self.start_scan(ScanRequest::subtree(selected));
        cx.notify();
    }

    fn reveal_selected_action(&mut self, cx: &mut Context<Self>) {
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

    fn confirm_delete_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn select_row(&mut self, node_id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        self.model.select(node_id);
        self.close_context_menu_state();
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn open_context_for_row(
        &mut self,
        node_id: NodeId,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenuState { node_id, position });
        self.model.set_context_target(Some(node_id));
        self.model.select(node_id);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn close_context_menu_state(&mut self) {
        self.context_menu = None;
        self.model.set_context_target(None);
    }

    fn dismiss_context_menu(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.context_menu.is_some() {
            self.close_context_menu_state();
            cx.notify();
        }
    }

    fn toggle_row(&mut self, node_id: NodeId, _: &mut Window, cx: &mut Context<Self>) {
        self.model.toggle_expanded(node_id);
        cx.notify();
    }

    fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected else {
            return;
        };

        self.model.toggle_expanded(selected);
        cx.notify();
    }

    fn jump_to_edge(&mut self, end: bool, cx: &mut Context<Self>) {
        let rows = self.model.visible_nodes();
        if let Some(row) = if end { rows.last() } else { rows.first() } {
            self.model.select(row.id);
            cx.notify();
        }
    }

    fn keyboard_menu_position(&self) -> gpui::Point<Pixels> {
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

        point(px(320.0), px(220.0 + (row_index.min(10) as f32 * 28.0)))
    }

    fn open_context_for_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.model.selected else {
            return;
        };

        self.open_context_for_row(selected, self.keyboard_menu_position(), window, cx);
    }

    fn invoke_context_reveal(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.reveal_selected_action(cx);
    }

    fn invoke_context_rescan_selection(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.rescan_selected_action(cx);
    }

    fn invoke_context_rescan_root(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_context_menu_state();
        self.rescan_root_action(cx);
    }

    fn invoke_context_delete(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete_action(window, cx);
    }

    fn handle_key_down(
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
            "r" if !event.keystroke.modifiers.control && !event.keystroke.modifiers.platform => {
                self.rescan_selected_action(cx)
            }
            "escape" | "esc" => self.close_context_menu_state(),
            "delete" => self.confirm_delete_action(window, cx),
            _ => return,
        }

        cx.notify();
    }

    fn render_action_button(
        &self,
        label: &'static str,
        enabled: bool,
        color: u32,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let mut button = div()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x374151))
            .bg(rgb(if enabled { color } else { 0x2b313b }))
            .text_color(rgb(0xf9fafb))
            .child(label);

        if enabled {
            button = button
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_click(event, window, cx)
                });
        }

        button
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = self.model.progress();
        let progress_fraction = progress.fraction();
        let root_text = self
            .model
            .active_root_path()
            .map(|path| shorten_path(path, 88))
            .unwrap_or_else(|| String::from("No root selected"));
        let duration = self
            .model
            .last_scan_duration()
            .map(format_duration)
            .unwrap_or_else(|| String::from("in progress"));

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(rgb(0x111827))
            .border_b_1()
            .border_color(rgb(0x1f2937))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(rgb(0xf9fafb))
                                    .child("Disk Analyzer"),
                            )
                            .child(div().text_color(rgb(0x9ca3af)).child(root_text)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.render_action_button(
                                "Choose Folder",
                                true,
                                0x0f766e,
                                cx.listener(Self::choose_directory),
                            ))
                            .child(self.render_action_button(
                                "Rescan Root",
                                self.model.active_root_path().is_some(),
                                0x1d4ed8,
                                cx.listener(Self::rescan_root),
                            ))
                            .child(self.render_action_button(
                                "Rescan Selection",
                                self.model.selected_path().is_some(),
                                0x7c3aed,
                                cx.listener(Self::rescan_selected),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(self.stat_card("Files", progress.files_scanned.to_string()))
                    .child(self.stat_card("Folders", progress.directories_scanned.to_string()))
                    .child(self.stat_card("Accumulated", format_bytes(progress.bytes_scanned)))
                    .child(self.stat_card("Last Run", duration)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .h(px(10.0))
                            .w_full()
                            .rounded_md()
                            .bg(rgb(0x1f2937))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(progress_fraction))
                                    .rounded_md()
                                    .bg(rgb(0x10b981)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_color(rgb(0xd1d5db))
                            .child(format!("Progress: {:.0}%", progress_fraction * 100.0))
                            .child(
                                progress
                                    .current_path
                                    .as_deref()
                                    .map(|path| shorten_path(path, 78))
                                    .unwrap_or_else(|| String::from("Idle")),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div().text_color(rgb(0xfbbf24)).child(
                            "Keyboard: Enter/Space toggle, Del deletes, Shift+F10 opens menu",
                        ),
                    )
                    .child(self.render_action_button(
                        self.model.sort_mode.label(),
                        true,
                        0x374151,
                        cx.listener(|this, _, _, cx| {
                            this.model.toggle_sort_mode();
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .text_color(rgb(0x9ca3af))
                    .child(self.model.status_message.clone()),
            )
    }

    fn stat_card(&self, label: &str, value: String) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x172033))
            .min_w(px(150.0))
            .child(div().text_color(rgb(0x9ca3af)).child(label.to_string()))
            .child(div().text_color(rgb(0xf9fafb)).child(value))
    }

    fn render_menu_item(
        &self,
        label: &'static str,
        accent: u32,
        enabled: bool,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let mut item = div()
            .px_3()
            .py_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(accent))
            .bg(rgb(if enabled { 0x0f172a } else { 0x111827 }))
            .text_color(rgb(if enabled { 0xf8fafc } else { 0x64748b }))
            .child(label);

        if enabled {
            item = item
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x1e293b)))
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_click(event, window, cx)
                });
        }

        item
    }

    fn render_context_menu(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.context_menu?;
        let selected_label = self
            .model
            .nodes
            .get(menu.node_id)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| String::from("Selection"));
        let has_selection = self.model.selected_path().is_some();
        let has_root = self.model.active_root_path().is_some();

        Some(
            anchored().position(menu.position).snap_to_window().child(
                div()
                    .w(px(220.0))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x334155))
                    .bg(rgb(0x020617))
                    .shadow_lg()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_color(rgb(0xfbbf24))
                            .child(format!("Actions for {selected_label}")),
                    )
                    .child(self.render_menu_item(
                        "Reveal in File Manager",
                        0x0ea5e9,
                        has_selection,
                        cx.listener(Self::invoke_context_reveal),
                    ))
                    .child(self.render_menu_item(
                        "Rescan Selected Subtree",
                        0x8b5cf6,
                        has_selection,
                        cx.listener(Self::invoke_context_rescan_selection),
                    ))
                    .child(self.render_menu_item(
                        "Rescan Root",
                        0x2563eb,
                        has_root,
                        cx.listener(Self::invoke_context_rescan_root),
                    ))
                    .child(self.render_menu_item(
                        "Delete",
                        0xdc2626,
                        has_selection,
                        cx.listener(Self::invoke_context_delete),
                    )),
            ),
        )
    }

    fn render_tree(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let row_count = self.model.visible_nodes().len();
        let focus_handle = self.focus_handle.clone();

        let mut tree = div()
            .flex()
            .flex_col()
            .size_full()
            .track_focus(&focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::dismiss_context_menu))
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(
                uniform_list(
                    "disk-tree",
                    row_count,
                    cx.processor(|this, range, _window, cx| {
                        let rows = this.model.visible_nodes();
                        let view = cx.entity().downgrade();
                        let mut elements = Vec::new();

                        for index in range {
                            if let Some(row) = rows.get(index as usize).cloned() {
                                let node_id = row.id;
                                let select_view = view.clone();
                                let context_view = view.clone();
                                let toggle_view = view.clone();
                                let indent = px((row.depth * 18) as f32);
                                let caret = if row.kind.is_directory() {
                                    if row.has_children {
                                        if row.expanded {
                                            "▾"
                                        } else {
                                            "▸"
                                        }
                                    } else {
                                        "▹"
                                    }
                                } else {
                                    "•"
                                };

                                let row_bg = if row.selected { 0x243144 } else { 0x0f172a };
                                let name_color = if row.has_error { 0xfca5a5 } else { 0xf9fafb };

                                let mut row_div = div()
                                    .id(index)
                                    .h(px(28.0))
                                    .w_full()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .px_3()
                                    .bg(rgb(row_bg))
                                    .border_b_1()
                                    .border_color(rgb(0x111827))
                                    .cursor_pointer()
                                    .on_click(move |_, window, cx| {
                                        let _ = select_view.update(cx, |this, cx| {
                                            this.select_row(node_id, window, cx)
                                        });
                                    })
                                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                        let _ = context_view.update(cx, |this, cx| {
                                            this.open_context_for_row(
                                                node_id,
                                                event.position,
                                                window,
                                                cx,
                                            )
                                        });
                                    });

                                if row.kind.is_directory() {
                                    row_div = row_div.on_mouse_down(
                                        MouseButton::Left,
                                        move |_, window, cx| {
                                            let _ = toggle_view.update(cx, |this, cx| {
                                                this.toggle_row(node_id, window, cx)
                                            });
                                        },
                                    );
                                }

                                elements.push(
                                    row_div
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .pl(indent)
                                                .child(div().text_color(rgb(0x60a5fa)).child(caret))
                                                .child(
                                                    div()
                                                        .text_color(rgb(name_color))
                                                        .child(row.name),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_color(rgb(0x94a3b8))
                                                        .child(shorten_path(&row.path, 42)),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(rgb(0xfbbf24))
                                                        .child(format_bytes(row.recursive_size)),
                                                ),
                                        ),
                                );
                            }
                        }

                        elements
                    }),
                )
                .h_full(),
            );

        if let Some(menu) = self.render_context_menu(cx) {
            tree = tree.child(menu);
        }

        tree
    }
}

impl Focusable for DiskAnalyzerApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DiskAnalyzerApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x020617))
            .child(self.render_header(cx))
            .child(self.render_tree(cx))
    }
}
