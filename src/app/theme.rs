use gpui::{App, Window};
use gpui_component::{IconName, Theme, ThemeMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemePreference {
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub(super) fn cycle(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub(super) fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Palette,
            Self::Light => IconName::Sun,
            Self::Dark => IconName::Moon,
        }
    }
}

pub(super) fn apply_theme_preference(
    preference: ThemePreference,
    window: &mut Window,
    cx: &mut App,
) {
    let mode = match preference {
        ThemePreference::System => ThemeMode::from(window.appearance()),
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
    };

    if Theme::global(cx).mode != mode {
        Theme::change(mode, Some(window), cx);
    }
}
