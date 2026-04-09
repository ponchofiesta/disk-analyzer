use gpui::{Window, WindowAppearance};
use gpui_component::IconName;

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

#[derive(Clone, Copy)]
pub(super) struct AppTheme {
    pub(super) app_bg: u32,
    pub(super) panel_bg: u32,
    pub(super) elevated_bg: u32,
    pub(super) elevated_alt_bg: u32,
    pub(super) menu_bg: u32,
    pub(super) border: u32,
    pub(super) border_subtle: u32,
    pub(super) text_primary: u32,
    pub(super) text_secondary: u32,
    pub(super) text_muted: u32,
    pub(super) accent: u32,
    pub(super) accent_soft: u32,
    pub(super) selection_bg: u32,
    pub(super) row_bg: u32,
    pub(super) row_hover: u32,
    pub(super) success: u32,
    pub(super) warning: u32,
    pub(super) danger: u32,
}

impl AppTheme {
    pub(super) fn from_window(window: &Window, preference: ThemePreference) -> Self {
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
