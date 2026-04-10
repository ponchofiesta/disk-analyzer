use gpui::{
    actions, anchored, div, prelude::*, px, rgb, App, Context, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Render, WeakEntity, Window, WindowBackgroundAppearance,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    menu::PopupMenu,
    progress::Progress,
    spinner::Spinner,
    table::{Column, ColumnSort, Table, TableDelegate, TableEvent, TableState},
    Disableable, Icon, IconName, Sizable, Size,
};

use crate::model::{NodeKind, SortMode, VisibleNode};
use crate::ui::{format_bytes, format_duration, format_modified_time, shorten_path, shorten_text};

use super::{theme::AppTheme, DiskAnalyzerApp};

actions!(
    results_table_menu,
    [
        RevealSelection,
        RescanSelection,
        RescanRoot,
        DeleteSelection
    ]
);

pub(super) type ResultsTableState = TableState<ResultsTableDelegate>;

#[derive(Clone)]
pub(super) struct ResultsTableDelegate {
    app: WeakEntity<DiskAnalyzerApp>,
    focus_handle: FocusHandle,
    theme_preference: super::theme::ThemePreference,
    rows: Vec<VisibleNode>,
    root_total_size: u64,
    columns: Vec<Column>,
}

impl ResultsTableDelegate {
    fn new(app: WeakEntity<DiskAnalyzerApp>, focus_handle: FocusHandle) -> Self {
        Self {
            app,
            focus_handle,
            theme_preference: super::theme::ThemePreference::System,
            rows: Vec::new(),
            root_total_size: 0,
            columns: Self::build_columns(SortMode::SizeDesc),
        }
    }

    fn build_columns(sort_mode: SortMode) -> Vec<Column> {
        vec![
            Column::new("tree", "Name")
                .width(px(520.0))
                .fixed_left()
                .sort(Self::column_sort(sort_mode, 0))
                .movable(false),
            Column::new("size", "Size")
                .width(px(120.0))
                .text_right()
                .sort(Self::column_sort(sort_mode, 1))
                .movable(false),
            Column::new("files", "Files")
                .width(px(90.0))
                .text_right()
                .sort(Self::column_sort(sort_mode, 2))
                .movable(false),
            Column::new("share", "Share")
                .width(px(190.0))
                .movable(false)
                .resizable(false)
                .selectable(false),
            Column::new("modified", "Modified")
                .width(px(150.0))
                .text_right()
                .sort(Self::column_sort(sort_mode, 4))
                .movable(false),
        ]
    }

    fn column_sort(sort_mode: SortMode, column_ix: usize) -> ColumnSort {
        match (column_ix, sort_mode) {
            (0, SortMode::NameAsc) => ColumnSort::Ascending,
            (0, SortMode::NameDesc) => ColumnSort::Descending,
            (1, SortMode::SizeAsc) => ColumnSort::Ascending,
            (1, SortMode::SizeDesc) => ColumnSort::Descending,
            (2, SortMode::FilesAsc) => ColumnSort::Ascending,
            (2, SortMode::FilesDesc) => ColumnSort::Descending,
            (4, SortMode::ModifiedAsc) => ColumnSort::Ascending,
            (4, SortMode::ModifiedDesc) => ColumnSort::Descending,
            _ => ColumnSort::Default,
        }
    }

    fn sync_from_app(&mut self, app: &DiskAnalyzerApp) {
        self.theme_preference = app.theme_preference;
        self.rows = app.model.visible_nodes();
        self.root_total_size = app.root_total_size();
        self.columns = Self::build_columns(app.model.sort_mode);
    }

    fn share_percent(&self, size: u64) -> f32 {
        if self.root_total_size == 0 {
            0.0
        } else {
            ((size as f64 / self.root_total_size as f64) * 100.0).clamp(0.0, 100.0) as f32
        }
    }
}

impl TableDelegate for ResultsTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };

        let next_sort = match col_ix {
            0 => SortMode::for_name(matches!(sort, ColumnSort::Descending)),
            1 => SortMode::for_size(matches!(sort, ColumnSort::Descending)),
            2 => SortMode::for_files(matches!(sort, ColumnSort::Descending)),
            4 => SortMode::for_modified(matches!(sort, ColumnSort::Descending)),
            _ => return,
        };

        let _ = app.update(cx, |app, cx| {
            app.model.set_sort_mode(next_sort);
            cx.notify();
        });

        let app = app.read(cx);
        self.sync_from_app(app);
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.rows[row_ix];
        let theme = AppTheme::from_window(window, self.theme_preference);
        let share_percent = self.share_percent(row.recursive_size);

        match col_ix {
            0 => {
                let node_id = row.id;
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
                let name_color = if row.has_error {
                    theme.danger
                } else {
                    theme.text_primary
                };

                let toggle = if row.kind.is_directory() && row.has_children {
                    let app = self.app.clone();
                    div()
                        .size(px(22.0))
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|style| style.bg(rgb(theme.elevated_alt_bg)))
                        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                            cx.stop_propagation();
                            let _ = app.update(cx, |app, cx| app.toggle_row(node_id, window, cx));
                        })
                        .child(
                            Icon::new(if row.expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .with_size(Size::XSmall)
                            .text_color(rgb(theme.text_muted)),
                        )
                        .into_any_element()
                } else {
                    div().size(px(22.0)).into_any_element()
                };

                div()
                    .h_full()
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
                                .child(shorten_text(&row.name, 42)),
                        ),
                    )
                    .into_any_element()
            }
            1 => div()
                .size_full()
                .text_right()
                .text_color(rgb(theme.text_secondary))
                .child(format_bytes(row.recursive_size))
                .into_any_element(),
            2 => div()
                .size_full()
                .text_right()
                .text_color(rgb(theme.text_secondary))
                .child(row.file_count.to_string())
                .into_any_element(),
            3 => div()
                .size_full()
                .flex()
                .items_center()
                .gap_2()
                .child(Progress::new().value(share_percent).h(px(8.0)).flex_1())
                .child(
                    div()
                        .min_w(px(48.0))
                        .text_right()
                        .text_color(rgb(theme.text_muted))
                        .child(format!("{share_percent:.1}%")),
                )
                .into_any_element(),
            4 => div()
                .size_full()
                .text_right()
                .text_color(rgb(if row.has_error {
                    theme.danger
                } else {
                    theme.text_muted
                }))
                .child(format_modified_time(row.modified_at))
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        if let Some(row) = self.rows.get(row_ix) {
            let node_id = row.id;
            let _ = self.app.update(cx, |app, cx| {
                app.model.set_context_target(Some(node_id));
                app.model.select(node_id);
                cx.notify();
            });
        }

        menu.action_context(self.focus_handle.clone())
            .menu_with_enable("Reveal in File Manager", Box::new(RevealSelection), true)
            .menu_with_enable("Rescan Selected Subtree", Box::new(RescanSelection), true)
            .menu_with_enable(
                "Rescan Root",
                Box::new(RescanRoot),
                self.root_total_size > 0,
            )
            .menu_with_enable("Delete", Box::new(DeleteSelection), true)
    }
}

impl DiskAnalyzerApp {
    fn ensure_results_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.results_table.is_some() {
            return;
        }

        let mut delegate =
            ResultsTableDelegate::new(cx.entity().downgrade(), self.focus_handle.clone());
        delegate.sync_from_app(self);

        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .loop_selection(false)
                .col_movable(false)
                .col_resizable(true)
                .col_selectable(false)
                .row_selectable(true)
                .sortable(true)
        });

        self.subscriptions
            .push(cx.subscribe(&table, |this, table, event, cx| match event {
                TableEvent::SelectRow(row_ix) => {
                    let node_id = table
                        .read(cx)
                        .delegate()
                        .rows
                        .get(*row_ix)
                        .map(|row| row.id);
                    if let Some(node_id) = node_id {
                        this.model.select(node_id);
                        this.close_context_menu_state();
                        cx.notify();
                    }
                }
                TableEvent::DoubleClickedRow(row_ix) => {
                    let node = table.read(cx).delegate().rows.get(*row_ix).cloned();
                    if let Some(node) = node {
                        this.model.select(node.id);
                        if node.kind.is_directory() {
                            this.model.toggle_expanded(node.id);
                        } else {
                            this.reveal_selected_action(cx);
                        }
                        cx.notify();
                    }
                }
                _ => {}
            }));

        self.results_table = Some(table);
    }

    fn sync_results_table(&mut self, cx: &mut Context<Self>) {
        let Some(table) = &self.results_table else {
            return;
        };

        table.update(cx, |table, cx| {
            table.delegate_mut().sync_from_app(self);
            table.refresh(cx);
            if let Some(selected) = self.model.selected {
                if let Some(row_ix) = table
                    .delegate()
                    .rows
                    .iter()
                    .position(|row| row.id == selected)
                {
                    table.set_selected_row(row_ix, cx);
                }
            }
        });
    }

    fn menu_reveal_action(&mut self, _: &RevealSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.reveal_selected_action(cx);
    }

    fn menu_rescan_selection_action(
        &mut self,
        _: &RescanSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rescan_selected_action(cx);
    }

    fn menu_rescan_root_action(&mut self, _: &RescanRoot, _: &mut Window, cx: &mut Context<Self>) {
        self.rescan_root_action(cx);
    }

    fn menu_delete_action(
        &mut self,
        _: &DeleteSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete_action(window, cx);
    }

    fn scan_state_label(&self) -> String {
        let duration = self
            .model
            .last_scan_duration()
            .map(format_duration)
            .unwrap_or_else(|| String::from("N/A"));
        match self.model.scan_state.as_ref() {
            Some(state) if !state.progress.finished => format!("Scanning ({duration})"),
            Some(state) if state.progress.finished => format!("Scan complete ({duration})"),
            Some(_) => String::from("Ready"),
            None => String::from("Idle"),
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
                        if self
                            .model
                            .scan_state
                            .as_ref()
                            .map_or(false, |state| !state.progress.finished)
                        {
                            div().child(
                                Spinner::new()
                                    .icon(IconName::LoaderCircle)
                                    .with_size(Size::Small)
                                    .color(rgb(theme.accent).into()),
                            )
                        } else {
                            div().child(
                                Icon::new(IconName::CircleCheck)
                                    .with_size(Size::Small)
                                    .text_color(rgb(self.scan_state_color(theme))),
                            )
                        },
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
        let root_text = self
            .model
            .active_root_path()
            .map(|path| shorten_path(path, 96))
            .unwrap_or_else(|| String::from("Choose a root folder to analyze disk usage."));

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
                        div(),
                    ),
            )
    }

    fn root_total_size(&self) -> u64 {
        self.model
            .root
            .and_then(|root| self.model.nodes.get(root))
            .map(|node| node.recursive_size)
            .unwrap_or(0)
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
        self.sync_results_table(cx);
        let table = self
            .results_table
            .as_ref()
            .expect("results table must be initialized before rendering")
            .clone();

        let mut tree = div()
            .flex()
            .flex_col()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::dismiss_context_menu))
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(
                Table::new(&table)
                    .stripe(true)
                    .bordered(false)
                    .with_size(Size::Small),
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
        self.ensure_results_table(window, cx);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.app_bg))
            .text_color(rgb(theme.text_primary))
            .on_action(cx.listener(Self::menu_reveal_action))
            .on_action(cx.listener(Self::menu_rescan_selection_action))
            .on_action(cx.listener(Self::menu_rescan_root_action))
            .on_action(cx.listener(Self::menu_delete_action))
            .child(self.render_header(cx, theme))
            .child(
                div().flex().flex_1().min_h_0().child(
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
                ),
            )
    }
}
