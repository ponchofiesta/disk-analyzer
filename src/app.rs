use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::Receiver;
use gpui::{
    px, size, App, AppContext, Application, AsyncApp, Bounds, Context, Entity, FocusHandle,
    Subscription, Timer, WeakEntity, WindowBounds, WindowOptions,
};
use gpui_component::{Root, TitleBar};
use gpui_component_assets::Assets;

use crate::model::AppModel;
use crate::scanner::{spawn_scan, ScanEvent, ScanHandle, ScanRequest};

mod actions;
mod theme;
mod views;

use self::theme::ThemePreference;

pub fn run() -> Result<()> {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
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
    results_table: Option<Entity<views::ResultsTableState>>,
    focus_handle: FocusHandle,
    theme_preference: ThemePreference,
    subscriptions: Vec<Subscription>,
}

impl DiskAnalyzerApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let app = Self {
            model: AppModel::default(),
            active_scan: None,
            receiver: None,
            results_table: None,
            focus_handle: cx.focus_handle(),
            theme_preference: ThemePreference::System,
            subscriptions: Vec::new(),
        };
        app.spawn_event_poller(cx);
        app
    }

    fn spawn_event_poller(&self, cx: &mut Context<Self>) {
        cx.spawn(|this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                loop {
                    Timer::after(Duration::from_millis(75)).await;
                    if this
                        .update(&mut cx, |this, cx: &mut Context<Self>| {
                            if this.process_scan_events() {
                                cx.notify();
                            }
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

    fn process_scan_events(&mut self) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };

        let mut changed = false;
        let mut clear_receiver = false;
        for event in receiver.try_iter() {
            changed = true;
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

        changed
    }

    fn start_scan(&mut self, request: ScanRequest) {
        if let Some(active_scan) = &self.active_scan {
            active_scan.cancel();
        }

        let scan_handle = spawn_scan(request);
        self.receiver = Some(scan_handle.receiver.clone());
        self.active_scan = Some(scan_handle);
    }
}
