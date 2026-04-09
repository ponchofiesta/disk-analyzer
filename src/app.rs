use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::Receiver;
use gpui::{
    anchored, div, point, prelude::*, px, rgb, size, uniform_list, App, Application, AsyncApp,
    Bounds, ClickEvent, Context, FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, PromptLevel, SharedString, Timer, WeakEntity, Window,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    progress::Progress,
    Disableable, Icon, IconName, Root, Sizable, Size,
};
use gpui_component_assets::Assets;

use crate::model::{AppModel, NodeId, NodeKind};
use crate::platform::{reveal_in_file_manager, trash_path};
use crate::scanner::{spawn_scan, ScanEvent, ScanHandle, ScanRequest};
use crate::ui::{format_bytes, format_duration, shorten_path};

pub fn run() -> Result<()> {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(DiskAnalyzerApp::new);
                cx.new(|cx| Root::new(view, window, cx))
            },
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
    theme_preference: ThemePreference,
}

#[derive(Clone, Copy)]
struct ContextMenuState {
    node_id: NodeId,
    position: gpui::Point<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemePreference {
    System,
    Light,
    Dark,
}

impl ThemePreference {
    fn cycle(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Palette,
            Self::Light => IconName::Sun,
            Self::Dark => IconName::Moon,
        }
    }
}

#[derive(Clone, Copy)]
struct AppTheme {
    app_bg: u32,
    panel_bg: u32,
    elevated_bg: u32,
    elevated_alt_bg: u32,
    menu_bg: u32,
    border: u32,
    border_subtle: u32,
    text_primary: u32,
    text_secondary: u32,
    text_muted: u32,
    accent: u32,
    accent_soft: u32,
    selection_bg: u32,
    row_bg: u32,
    row_hover: u32,
    success: u32,
    warning: u32,
    danger: u32,
}

impl AppTheme {
    fn from_window(window: &Window, preference: ThemePreference) -> Self {
        let dark = match preference {
            ThemePreference::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            ThemePreference::Light => false,
            ThemePreference::Dark => true,
        };

        if dark {
            Self {
                app_bg: 0x16171a,
                panel_bg: 0x1d1f23,
                elevated_bg: 0x23262b,
                elevated_alt_bg: 0x2b3036,
                menu_bg: 0x25292f,
                border: 0x3b4149,
                border_subtle: 0x2c3138,
                text_primary: 0xf5f7fa,
                text_secondary: 0xcdd4de,
                text_muted: 0x8f98a5,
                accent: 0x3b82f6,
                accent_soft: 0x12284a,
                selection_bg: 0x1a3156,
                row_bg: 0x1d1f23,
                row_hover: 0x262b31,
                success: 0x22c55e,
                warning: 0xf59e0b,
                danger: 0xef4444,
            }
        } else {
            Self {
                app_bg: 0xf3f5f8,
                panel_bg: 0xffffff,
                elevated_bg: 0xffffff,
                elevated_alt_bg: 0xf7f8fa,
                menu_bg: 0xffffff,
                border: 0xd7dde5,
                border_subtle: 0xe7ebf0,
                text_primary: 0x111827,
                text_secondary: 0x334155,
                text_muted: 0x64748b,
                accent: 0x2563eb,
                accent_soft: 0xe8f0ff,
                selection_bg: 0xeaf2ff,
                row_bg: 0xffffff,
                row_hover: 0xf7f8fa,
                success: 0x16a34a,
                warning: 0xd97706,
                danger: 0xdc2626,
            }
        }
    }
}

impl DiskAnalyzerApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let app = Self {
            model: AppModel::default(),
            active_scan: None,
            receiver: None,
            focus_handle: cx.focus_handle(),
            context_menu: None,
            theme_preference: ThemePreference::System,
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

    fn choose_directory_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn choose_directory_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choose_directory_impl(window, cx);
    }

    fn rescan_root_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn rescan_selected_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn reveal_selected_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.reveal_selected_action(cx);
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

    fn delete_selected_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete_action(window, cx);
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

    fn toggle_theme_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_preference = self.theme_preference.cycle();
        cx.notify();
    }

    fn toggle_sort_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.model.toggle_sort_mode();
        cx.notify();
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

        point(px(340.0), px(250.0 + (row_index.min(10) as f32 * 32.0)))
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

    fn selected_children_count(&self) -> usize {
        self.model
            .selected_node()
            .map(|node| {
                node.children
                    .iter()
                    .filter(|&&child| {
                        self.model
                            .nodes
                            .get(child)
                            .is_some_and(|child| !child.removed)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    fn selection_kind_label(kind: NodeKind) -> &'static str {
        match kind {
            NodeKind::Directory => "Directory",
            NodeKind::File => "File",
            NodeKind::Symlink => "Symlink",
            NodeKind::Other => "Other",
        }
    }

    fn scan_state_label(&self) -> &'static str {
        match self.model.scan_state.as_ref() {
            Some(state) if !state.progress.finished => "Scanning",
            Some(_) => "Ready",
            None => "Idle",
        }
    }

    fn scan_state_color(&self, theme: AppTheme) -> u32 {
        match self.model.scan_state.as_ref() {
            Some(state) if !state.progress.finished => theme.accent,
            Some(_) => theme.success,
            None => theme.text_muted,
        }
    }

    fn render_status_pill(&self, theme: AppTheme) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(rgb(theme.accent_soft))
            .border_1()
            .border_color(rgb(theme.border_subtle))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded_full()
                            .bg(rgb(self.scan_state_color(theme))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme.text_secondary))
                            .child(self.scan_state_label()),
                    ),
            )
    }

    fn render_metric_card(
        &self,
        icon: IconName,
        label: &str,
        value: String,
        tone: u32,
        theme: AppTheme,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .min_w(px(170.0))
            .rounded_lg()
            .bg(rgb(theme.elevated_bg))
            .border_1()
            .border_color(rgb(theme.border_subtle))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(theme.accent_soft))
                            .child(Icon::new(icon).with_size(Size::Small).text_color(rgb(tone))),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme.text_muted))
                            .child(label.to_string()),
                    ),
            )
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(theme.text_primary))
                    .child(value),
            )
    }

    fn action_button(
        id: &'static str,
        label: impl Into<SharedString>,
        icon: IconName,
        enabled: bool,
        primary: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Button {
        let button = Button::new(id)
            .label(label)
            .icon(icon)
            .with_size(Size::Small)
            .compact()
            .disabled(!enabled)
            .on_click(on_click);

        if primary {
            button.primary()
        } else {
            button.outline()
        }
    }

    fn render_header(&mut self, cx: &mut Context<Self>, theme: AppTheme) -> impl IntoElement {
        let progress = self.model.progress();
        let root_text = self
            .model
            .active_root_path()
            .map(|path| shorten_path(path, 96))
            .unwrap_or_else(|| String::from("Choose a root folder to analyze disk usage."));
        let duration = self
            .model
            .last_scan_duration()
            .map(format_duration)
            .unwrap_or_else(|| String::from("not started"));

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(rgb(theme.panel_bg))
            .border_b_1()
            .border_color(rgb(theme.border_subtle))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .size(px(40.0))
                                            .rounded_lg()
                                            .bg(rgb(theme.accent_soft))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                Icon::new(IconName::ChartPie)
                                                    .with_size(Size::Medium)
                                                    .text_color(rgb(theme.accent)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xl()
                                                    .text_color(rgb(theme.text_primary))
                                                    .child("Disk Analyzer"),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(theme.text_secondary))
                                                    .child("Fast, live disk usage inspection with safe actions."),
                                            ),
                                    ),
                            )
                            .child(div().text_color(rgb(theme.text_muted)).child(root_text)),
                    )
                    .child(self.render_status_pill(theme)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .flex_wrap()
                    .child(Self::action_button(
                        "choose-folder",
                        "Choose Folder",
                        IconName::FolderOpen,
                        true,
                        true,
                        cx.listener(Self::choose_directory_click),
                    ))
                    .child(Self::action_button(
                        "rescan-root",
                        "Rescan Root",
                        IconName::Redo,
                        self.model.active_root_path().is_some(),
                        false,
                        cx.listener(Self::rescan_root_click),
                    ))
                    .child(Self::action_button(
                        "rescan-selection",
                        "Rescan Selection",
                        IconName::Replace,
                        self.model.selected_path().is_some(),
                        false,
                        cx.listener(Self::rescan_selected_click),
                    ))
                    .child(Self::action_button(
                        "reveal-selection",
                        "Reveal",
                        IconName::ExternalLink,
                        self.model.selected_path().is_some(),
                        false,
                        cx.listener(Self::reveal_selected_click),
                    ))
                    .child(
                        Button::new("theme-toggle")
                            .label(format!("Theme: {}", self.theme_preference.label()))
                            .icon(self.theme_preference.icon())
                            .with_size(Size::Small)
                            .compact()
                            .outline()
                            .on_click(cx.listener(Self::toggle_theme_click)),
                    )
                    .child(
                        Button::new("sort-toggle")
                            .label(self.model.sort_mode.label())
                            .icon(match self.model.sort_mode {
                                crate::model::SortMode::SizeDesc => IconName::ChartPie,
                                crate::model::SortMode::NameAsc => IconName::ALargeSmall,
                            })
                            .with_size(Size::Small)
                            .compact()
                            .outline()
                            .on_click(cx.listener(Self::toggle_sort_click)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_wrap()
                    .child(self.render_metric_card(
                        IconName::File,
                        "Files scanned",
                        progress.files_scanned.to_string(),
                        theme.accent,
                        theme,
                    ))
                    .child(self.render_metric_card(
                        IconName::Folder,
                        "Folders scanned",
                        progress.directories_scanned.to_string(),
                        theme.warning,
                        theme,
                    ))
                    .child(self.render_metric_card(
                        IconName::ChartPie,
                        "Bytes observed",
                        format_bytes(progress.bytes_scanned),
                        theme.success,
                        theme,
                    ))
                    .child(self.render_metric_card(
                        IconName::Calendar,
                        "Last run",
                        duration,
                        theme.text_secondary,
                        theme,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(theme.elevated_alt_bg))
                    .border_1()
                    .border_color(rgb(theme.border_subtle))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_color(rgb(theme.text_secondary))
                                    .child(format!("Progress {:.0}%", progress.fraction() * 100.0)),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme.text_muted))
                                    .child(
                                        progress
                                            .current_path
                                            .as_deref()
                                            .map(|path| shorten_path(path, 88))
                                            .unwrap_or_else(|| String::from("Idle")),
                                    ),
                            ),
                    )
                    .child(
                        Progress::new()
                            .value(progress.fraction() * 100.0)
                            .h(px(10.0)),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme.text_muted))
                            .child(self.model.status_message.clone()),
                    ),
            )
    }

    fn render_info_row(&self, label: &str, value: String, theme: AppTheme) -> impl IntoElement {
        div()
            .flex()
            .justify_between()
            .gap_3()
            .py_1()
            .child(
                div()
                    .text_color(rgb(theme.text_muted))
                    .child(label.to_string()),
            )
            .child(
                div()
                    .text_right()
                    .text_color(rgb(theme.text_primary))
                    .child(value),
            )
    }

    fn render_details_pane(&mut self, cx: &mut Context<Self>, theme: AppTheme) -> impl IntoElement {
        let selected = self.model.selected_node().cloned();
        let warning_text = self.model.warnings.last().cloned();

        div()
            .w(px(340.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(rgb(theme.panel_bg))
            .border_l_1()
            .border_color(rgb(theme.border_subtle))
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
                                    .text_color(rgb(theme.text_primary))
                                    .child("Details"),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme.text_muted))
                                    .child("Selection context and quick actions"),
                            ),
                    )
                    .child(Icon::new(IconName::Inspector).with_size(Size::Small).text_color(rgb(theme.text_muted))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(theme.elevated_bg))
                    .border_1()
                    .border_color(rgb(theme.border_subtle))
                    .when_some(selected.clone(), |this, node| {
                        this.child(
                            div()
                                .flex()
                                .items_start()
                                .gap_3()
                                .child(
                                    div()
                                        .size(px(36.0))
                                        .rounded_lg()
                                        .bg(rgb(theme.accent_soft))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(match node.kind {
                                                NodeKind::Directory => IconName::Folder,
                                                NodeKind::File => IconName::File,
                                                NodeKind::Symlink => IconName::ExternalLink,
                                                NodeKind::Other => IconName::Frame,
                                            })
                                            .with_size(Size::Small)
                                            .text_color(rgb(theme.accent)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_color(rgb(theme.text_primary))
                                                .child(node.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_color(rgb(theme.text_muted))
                                                .child(shorten_path(&node.path, 44)),
                                        ),
                                ),
                        )
                        .child(self.render_info_row(
                            "Type",
                            Self::selection_kind_label(node.kind).to_string(),
                            theme,
                        ))
                        .child(self.render_info_row(
                            "Size",
                            format_bytes(node.recursive_size),
                            theme,
                        ))
                        .child(self.render_info_row(
                            "Children",
                            self.selected_children_count().to_string(),
                            theme,
                        ))
                        .child(self.render_info_row(
                            "Depth",
                            node.depth.to_string(),
                            theme,
                        ))
                        .when_some(node.last_error.clone(), |this, error| {
                            this.child(
                                div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(rgb(theme.accent_soft))
                                    .border_1()
                                    .border_color(rgb(theme.border_subtle))
                                    .child(
                                        div()
                                            .text_color(rgb(theme.danger))
                                            .child(error),
                                    ),
                            )
                        })
                    })
                    .when(selected.is_none(), |this| {
                        this.child(
                            div()
                                .text_color(rgb(theme.text_muted))
                                .child("Select an item in the tree to inspect it and run focused actions."),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(theme.elevated_bg))
                    .border_1()
                    .border_color(rgb(theme.border_subtle))
                    .child(
                        div()
                            .text_color(rgb(theme.text_secondary))
                            .child("Quick Actions"),
                    )
                    .child(Self::action_button(
                        "details-reveal",
                        "Reveal in file manager",
                        IconName::ExternalLink,
                        self.model.selected_path().is_some(),
                        false,
                        cx.listener(Self::reveal_selected_click),
                    ))
                    .child(Self::action_button(
                        "details-rescan",
                        "Rescan selected subtree",
                        IconName::Redo2,
                        self.model.selected_path().is_some(),
                        false,
                        cx.listener(Self::rescan_selected_click),
                    ))
                    .child(
                        Button::new("details-delete")
                            .label("Move to Trash")
                            .icon(IconName::Delete)
                            .with_size(Size::Small)
                            .compact()
                            .danger()
                            .disabled(self.model.selected_path().is_none())
                            .on_click(cx.listener(Self::delete_selected_click)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(theme.elevated_bg))
                    .border_1()
                    .border_color(rgb(theme.border_subtle))
                    .child(
                        div()
                            .text_color(rgb(theme.text_secondary))
                            .child("Hints"),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme.text_muted))
                            .child("Arrow keys move selection. Enter expands folders or reveals files. Shift+F10 opens the context menu."),
                    )
                    .when_some(warning_text, |this, warning| {
                        this.child(
                            div()
                                .p_2()
                                .rounded_md()
                                .bg(rgb(theme.accent_soft))
                                .border_1()
                                .border_color(rgb(theme.border_subtle))
                                .child(
                                    div()
                                        .text_color(rgb(theme.warning))
                                        .child(warning),
                                ),
                        )
                    }),
            )
    }

    fn render_menu_item(
        &self,
        label: &'static str,
        icon: IconName,
        accent: u32,
        enabled: bool,
        theme: AppTheme,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let mut item = div()
            .px_3()
            .py_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(if enabled { accent } else { theme.border_subtle }))
            .bg(rgb(if enabled {
                theme.elevated_bg
            } else {
                theme.elevated_alt_bg
            }))
            .text_color(rgb(if enabled {
                theme.text_primary
            } else {
                theme.text_muted
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(icon).with_size(Size::Small))
                    .child(label),
            );

        if enabled {
            item = item
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme.row_hover)))
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_click(event, window, cx)
                });
        }

        item
    }

    fn render_context_menu(
        &mut self,
        cx: &mut Context<Self>,
        theme: AppTheme,
    ) -> Option<impl IntoElement> {
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
                    .w(px(240.0))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(theme.border))
                    .bg(rgb(theme.menu_bg))
                    .shadow_lg()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_color(rgb(theme.text_secondary))
                            .child(format!("Actions for {selected_label}")),
                    )
                    .child(self.render_menu_item(
                        "Reveal in File Manager",
                        IconName::ExternalLink,
                        theme.accent,
                        has_selection,
                        theme,
                        cx.listener(Self::invoke_context_reveal),
                    ))
                    .child(self.render_menu_item(
                        "Rescan Selected Subtree",
                        IconName::Redo2,
                        theme.accent,
                        has_selection,
                        theme,
                        cx.listener(Self::invoke_context_rescan_selection),
                    ))
                    .child(self.render_menu_item(
                        "Rescan Root",
                        IconName::Redo,
                        theme.accent,
                        has_root,
                        theme,
                        cx.listener(Self::invoke_context_rescan_root),
                    ))
                    .child(self.render_menu_item(
                        "Delete",
                        IconName::Delete,
                        theme.danger,
                        has_selection,
                        theme,
                        cx.listener(Self::invoke_context_delete),
                    )),
            ),
        )
    }

    fn render_tree(&mut self, cx: &mut Context<Self>, theme: AppTheme) -> impl IntoElement {
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
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_2()
                    .bg(rgb(theme.elevated_alt_bg))
                    .border_b_1()
                    .border_color(rgb(theme.border_subtle))
                    .child(div().text_color(rgb(theme.text_secondary)).child("Tree"))
                    .child(
                        div()
                            .text_color(rgb(theme.text_muted))
                            .child("Right click for actions"),
                    ),
            )
            .child(
                uniform_list(
                    "disk-tree",
                    row_count,
                    cx.processor(move |this, range, _window, cx| {
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
                                let icon = match row.kind {
                                    NodeKind::Directory => {
                                        if row.expanded {
                                            IconName::FolderOpen
                                        } else {
                                            IconName::FolderClosed
                                        }
                                    }
                                    NodeKind::File => IconName::File,
                                    NodeKind::Symlink => IconName::ExternalLink,
                                    NodeKind::Other => IconName::Frame,
                                };
                                let row_bg = if row.selected {
                                    theme.selection_bg
                                } else {
                                    theme.row_bg
                                };
                                let name_color = if row.has_error {
                                    theme.danger
                                } else {
                                    theme.text_primary
                                };

                                let row_div = div()
                                    .id(index)
                                    .h(px(44.0))
                                    .w_full()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .px_3()
                                    .bg(rgb(row_bg))
                                    .border_b_1()
                                    .border_color(rgb(theme.border_subtle))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(theme.row_hover)))
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

                                let toggle = if row.kind.is_directory() && row.has_children {
                                    let toggle_view = toggle_view.clone();
                                    div()
                                        .size(px(22.0))
                                        .rounded_sm()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .hover(|style| style.bg(rgb(theme.elevated_alt_bg)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |event, window, cx| {
                                                cx.stop_propagation();
                                                let _ = toggle_view.update(cx, |this, cx| {
                                                    this.toggle_row(node_id, window, cx)
                                                });
                                                let _ = event;
                                            },
                                        )
                                        .child(
                                            Icon::new(if row.expanded {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            })
                                            .with_size(Size::XSmall)
                                            .text_color(rgb(theme.text_muted)),
                                        )
                                } else {
                                    div().size(px(22.0))
                                };

                                elements.push(
                                    row_div
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .pl(indent)
                                                .child(toggle)
                                                .child(
                                                    Icon::new(icon)
                                                        .with_size(Size::Small)
                                                        .text_color(rgb(theme.accent)),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .gap_0p5()
                                                        .child(
                                                            div()
                                                                .text_color(rgb(name_color))
                                                                .child(row.name),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_color(rgb(theme.text_muted))
                                                                .child(shorten_path(&row.path, 56)),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_3()
                                                .child(
                                                    div()
                                                        .text_color(rgb(theme.text_secondary))
                                                        .child(format_bytes(row.recursive_size)),
                                                )
                                                .when(row.has_error, |this| {
                                                    this.child(
                                                        div()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded_full()
                                                            .bg(rgb(theme.accent_soft))
                                                            .text_color(rgb(theme.danger))
                                                            .child("Error"),
                                                    )
                                                }),
                                        ),
                                );
                            }
                        }

                        elements
                    }),
                )
                .h_full(),
            );

        if let Some(menu) = self.render_context_menu(cx, theme) {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        let theme = AppTheme::from_window(window, self.theme_preference);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.app_bg))
            .text_color(rgb(theme.text_primary))
            .child(self.render_header(cx, theme))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div().flex_1().min_w_0().p_3().child(
                            div()
                                .size_full()
                                .rounded_lg()
                                .overflow_hidden()
                                .bg(rgb(theme.panel_bg))
                                .border_1()
                                .border_color(rgb(theme.border_subtle))
                                .child(self.render_tree(cx, theme)),
                        ),
                    )
                    .child(self.render_details_pane(cx, theme)),
            )
    }
}
