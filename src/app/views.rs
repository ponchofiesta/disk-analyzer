use gpui::{
    anchored, div, prelude::*, px, rgb, App, Context, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Render, Window, WindowBackgroundAppearance,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    spinner::Spinner,
    Disableable, Icon, IconName, Sizable, Size,
};

use crate::model::NodeKind;
use crate::ui::{format_bytes, format_duration, shorten_path};

use super::{theme::AppTheme, DiskAnalyzerApp};

impl DiskAnalyzerApp {
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
        label: impl Into<gpui::SharedString>,
        icon: IconName,
        enabled: bool,
        primary: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
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
        let is_scanning = self
            .model
            .scan_state
            .as_ref()
            .is_some_and(|state| !state.progress.finished);
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
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        if is_scanning {
                                            div().child(
                                                Spinner::new()
                                                    .icon(IconName::LoaderCircle)
                                                    .with_size(Size::Small)
                                                    .color(rgb(theme.accent).into()),
                                            )
                                        } else {
                                            div().child(
                                                Icon::new(if progress.finished {
                                                    IconName::Check
                                                } else {
                                                    IconName::Info
                                                })
                                                .with_size(Size::Small)
                                                .text_color(rgb(if progress.finished {
                                                    theme.success
                                                } else {
                                                    theme.text_muted
                                                })),
                                            )
                                        },
                                    )
                                    .child(
                                        div()
                                            .text_color(rgb(theme.text_secondary))
                                            .child(if is_scanning {
                                                "Scanning"
                                            } else if progress.finished {
                                                "Scan complete"
                                            } else {
                                                "Idle"
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme.text_secondary))
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
                    .child(
                        Icon::new(IconName::Inspector)
                            .with_size(Size::Small)
                            .text_color(rgb(theme.text_muted)),
                    ),
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
                                    .child(div().text_color(rgb(theme.danger)).child(error)),
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
                    .child(div().text_color(rgb(theme.text_secondary)).child("Quick Actions"))
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
                    .child(div().text_color(rgb(theme.text_secondary)).child("Hints"))
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
                                .child(div().text_color(rgb(theme.warning)).child(warning)),
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
                gpui::uniform_list(
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
                                            move |_event, window, cx| {
                                                cx.stop_propagation();
                                                let _ = toggle_view.update(cx, |this, cx| {
                                                    this.toggle_row(node_id, window, cx)
                                                });
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
                                                    div().flex().flex_col().gap_0p5().child(
                                                        div()
                                                            .text_color(rgb(name_color))
                                                            .child(row.name),
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
