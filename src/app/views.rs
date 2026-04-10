use gpui::{
    actions, div, prelude::*, px, App, Context, FocusHandle, Focusable, MouseButton, Render,
    WeakEntity, Window, WindowBackgroundAppearance,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    menu::PopupMenu,
    progress::Progress,
    spinner::Spinner,
    table::{Column, ColumnSort, Table, TableDelegate, TableEvent, TableState},
    ActiveTheme, Icon, IconName, Sizable, Size, TitleBar,
};

use crate::model::{NodeKind, SortMode, VisibleNode};
use crate::ui::{format_bytes, format_duration, format_modified_time, shorten_path, shorten_text};

use super::{theme::apply_theme_preference, DiskAnalyzerApp};

actions!(
    results_table_menu,
    [RevealSelection, RescanSelection, DeleteSelection]
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
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.rows[row_ix];
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
                let toggle = if row.kind.is_directory() && row.has_children {
                    let app = self.app.clone();
                    div()
                        .size(px(22.0))
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|style| style.bg(_cx.theme().secondary))
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
                            .text_color(_cx.theme().muted_foreground),
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
                            .text_color(if row.has_error {
                                _cx.theme().danger
                            } else {
                                _cx.theme().primary
                            }),
                    )
                    .child(
                        div().flex().flex_col().gap_0p5().child(
                            div()
                                .text_color(if row.has_error {
                                    _cx.theme().danger
                                } else {
                                    _cx.theme().foreground
                                })
                                .child(shorten_text(&row.name, 42)),
                        ),
                    )
                    .into_any_element()
            }
            1 => div()
                .size_full()
                .text_right()
                .text_color(_cx.theme().foreground)
                .child(format_bytes(row.recursive_size))
                .into_any_element(),
            2 => div()
                .size_full()
                .text_right()
                .text_color(_cx.theme().foreground)
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
                        .text_color(_cx.theme().muted_foreground)
                        .child(format!("{share_percent:.1}%")),
                )
                .into_any_element(),
            4 => div()
                .size_full()
                .text_right()
                .text_color(if row.has_error {
                    _cx.theme().danger
                } else {
                    _cx.theme().muted_foreground
                })
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
            .menu_with_enable("Rescan", Box::new(RescanSelection), true)
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

    fn scan_state_color(&self, cx: &App) -> gpui::Hsla {
        match self.model.scan_state.as_ref() {
            Some(state) if !state.progress.finished => cx.theme().primary,
            Some(_) => cx.theme().success,
            None => cx.theme().muted_foreground,
        }
    }

    fn render_status_pill(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
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
                                    .color(cx.theme().primary),
                            )
                        } else {
                            div().child(
                                Icon::new(IconName::CircleCheck)
                                    .with_size(Size::Small)
                                    .text_color(self.scan_state_color(cx)),
                            )
                        },
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(self.scan_state_label()),
                    ),
            )
    }

    fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .h_full()
                .gap_3()
                .pr_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .min_w_0()
                        .child(
                            div()
                                .size(px(24.0))
                                .rounded_md()
                                .bg(cx.theme().secondary)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::ChartPie)
                                        .with_size(Size::XSmall)
                                        .text_color(cx.theme().primary),
                                ),
                        )
                        .child(
                            div().flex().min_w_0().child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().foreground)
                                    .child("Disk Analyzer"),
                            ),
                        ),
                )
                .child(
                    Button::new("titlebar-theme-toggle")
                        .label(format!("Theme: {}", self.theme_preference.label()))
                        .icon(self.theme_preference.icon())
                        .with_size(Size::Small)
                        .compact()
                        .outline()
                        .on_click(cx.listener(Self::toggle_theme_click)),
                ),
        )
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

        div()
            .flex()
            .justify_between()
            .gap_4()
            .p_4()
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(if is_scanning {
                        Button::new("cancel-scan")
                            .label("Cancel")
                            .icon(IconName::Close)
                            .with_size(Size::Medium)
                            .compact()
                            .danger()
                            .on_click(cx.listener(Self::cancel_scan_click))
                            .into_any_element()
                    } else {
                        Button::new("choose-folder")
                            .label("Choose Folder")
                            .icon(IconName::FolderOpen)
                            .with_size(Size::Medium)
                            .compact()
                            .primary()
                            .on_click(cx.listener(Self::choose_directory_click))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(root_text),
                    ),
            )
            .child(self.render_status_pill(cx))
    }

    fn root_total_size(&self) -> u64 {
        self.model
            .root
            .and_then(|root| self.model.nodes.get(root))
            .map(|node| node.recursive_size)
            .unwrap_or(0)
    }

    fn render_tree(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_results_table(cx);
        let table = self
            .results_table
            .as_ref()
            .expect("results table must be initialized before rendering")
            .clone();

        let tree = div()
            .flex()
            .flex_col()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(
                Table::new(&table)
                    .stripe(true)
                    .bordered(false)
                    .with_size(Size::Small),
            );

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
        apply_theme_preference(self.theme_preference, window, cx);
        self.ensure_results_table(window, cx);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::menu_reveal_action))
            .on_action(cx.listener(Self::menu_rescan_selection_action))
            .on_action(cx.listener(Self::menu_delete_action))
            .child(self.render_title_bar(cx))
            .child(self.render_header(cx))
            .child(
                div().flex().flex_1().min_h_0().child(
                    div().flex_1().min_w_0().p_3().child(
                        div()
                            .size_full()
                            .rounded_lg()
                            .overflow_hidden()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(self.render_tree(cx)),
                    ),
                ),
            )
    }
}
