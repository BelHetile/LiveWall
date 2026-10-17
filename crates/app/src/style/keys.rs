use crate::i18n::key;
use crate::style::color::{Rgba, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemColor {
    Label,
    SecondaryLabel,
    TertiaryLabel,
    QuaternaryLabel,
    Separator,
    WindowBackground,
    UnderPageBackground,
    ControlBackground,
    QuaternaryFill,
    ControlAccent,
    TextOnAccent,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Gray,
}

impl SystemColor {
    pub const ALL: [Self; 19] = [
        Self::Label,
        Self::SecondaryLabel,
        Self::TertiaryLabel,
        Self::QuaternaryLabel,
        Self::Separator,
        Self::WindowBackground,
        Self::UnderPageBackground,
        Self::ControlBackground,
        Self::QuaternaryFill,
        Self::ControlAccent,
        Self::TextOnAccent,
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Gray,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fallback {
    System(SystemColor),
    Neutral(SystemColor),
    Accent,
    AccentAlpha(f64),
    OnAccent,
    Key(Key),
    Fixed(Rgba, Rgba),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Base,
    Window,
    Sidebar,
    Toolbar,
    Grid,
    Chips,
    Downloads,
    Inspector,
    Settings,
    Picker,
    Fields,
}

impl Group {
    pub const ALL: [Self; 11] = [
        Self::Base,
        Self::Window,
        Self::Sidebar,
        Self::Toolbar,
        Self::Grid,
        Self::Chips,
        Self::Downloads,
        Self::Inspector,
        Self::Settings,
        Self::Picker,
        Self::Fields,
    ];

    pub fn title(self) -> &'static str {
        return match self {
            Self::Base => key("Base Colors"),
            Self::Window => key("Window"),
            Self::Sidebar => key("Sidebar"),
            Self::Toolbar => key("Toolbar"),
            Self::Grid => key("Wallpapers"),
            Self::Chips => key("Chips"),
            Self::Downloads => key("Download Status"),
            Self::Inspector => key("Inspector"),
            Self::Settings => key("Settings"),
            Self::Picker => key("Icon Picker"),
            Self::Fields => key("Fields"),
        };
    }

    pub fn keys(self) -> impl Iterator<Item = Key> {
        return Key::ALL.into_iter().filter(move |key| key.group() == self);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Canvas,
    Chrome,
    Surface,
    SurfaceRaised,
    Text,
    TextSecondary,
    TextTertiary,
    Hairline,
    Selection,
    SelectionSoft,
    OnAccent,
    Shadow,
    GlassTint,
    StatusOk,
    StatusWarn,
    StatusError,
    StatusOff,
    Destructive,
    WindowCanvas,
    SidebarIcon,
    SidebarCount,
    ToolbarPill,
    ToolbarPillHover,
    ToolbarSeparator,
    GridTile,
    GridTileBorder,
    GridRing,
    GridTitle,
    GridTitleSelected,
    GridMeta,
    GridHighlight,
    GridProgress,
    GridShade,
    GridShadow,
    GridEmpty,
    ChipFill,
    ChipText,
    ChipActive,
    PillFill,
    PillBar,
    PillTrack,
    InspectorBackdrop,
    InspectorPreviewBorder,
    InspectorSlider,
    InspectorSegment,
    SettingsCard,
    SettingsSeparator,
    SettingsRecorder,
    SettingsRecorderActive,
    SettingsSlider,
    PickerSelected,
    PickerDivider,
    FieldFill,
    FieldText,
}

pub struct Spec {
    pub key: Key,
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub role: Role,
    pub fallback: Fallback,
}

const fn spec(key: Key, id: &'static str, group: Group, title: &'static str, role: Role, fallback: Fallback) -> Spec {
    return Spec { key, id, group, title, role, fallback };
}

const SCRIM: Rgba = Rgba::new(0, 0, 0, 115);
const SHADOW_LIGHT: Rgba = Rgba::new(0, 0, 0, 36);
const SHADOW_DARK: Rgba = Rgba::new(0, 0, 0, 102);

use Fallback::{Accent, AccentAlpha, Fixed, Neutral, OnAccent, System};
use Group::{Base, Chips, Downloads, Fields, Grid, Inspector, Picker, Settings, Sidebar, Toolbar, Window};
use Role::{Content, Line, Shadow, Surface};

const SPECS: [Spec; 54] = [
    spec(Key::Canvas, "canvas", Base, key("Canvas"), Surface, Neutral(SystemColor::UnderPageBackground)),
    spec(Key::Chrome, "chrome", Base, key("Chrome"), Surface, Neutral(SystemColor::WindowBackground)),
    spec(Key::Surface, "surface", Base, key("Card"), Surface, System(SystemColor::QuaternaryFill)),
    spec(
        Key::SurfaceRaised,
        "surface.raised",
        Base,
        key("Raised Card"),
        Surface,
        Neutral(SystemColor::ControlBackground),
    ),
    spec(Key::Text, "text", Base, key("Text"), Content, System(SystemColor::Label)),
    spec(
        Key::TextSecondary,
        "text.secondary",
        Base,
        key("Secondary Text"),
        Content,
        System(SystemColor::SecondaryLabel),
    ),
    spec(Key::TextTertiary, "text.tertiary", Base, key("Tertiary Text"), Content, System(SystemColor::TertiaryLabel)),
    spec(Key::Hairline, "hairline", Base, key("Hairline"), Line, System(SystemColor::Separator)),
    spec(Key::Selection, "selection", Base, key("Selection"), Content, Accent),
    spec(Key::SelectionSoft, "selection.soft", Base, key("Soft Selection"), Surface, AccentAlpha(0.16)),
    spec(Key::OnAccent, "on_accent", Base, key("Text on Accent"), Content, OnAccent),
    spec(Key::Shadow, "shadow", Base, key("Shadow"), Shadow, Fixed(SHADOW_LIGHT, SHADOW_DARK)),
    spec(Key::GlassTint, "glass.tint", Base, key("Glass Tint"), Surface, AccentAlpha(0.2)),
    spec(Key::StatusOk, "status.ok", Base, key("Good Status"), Content, System(SystemColor::Green)),
    spec(Key::StatusWarn, "status.warn", Base, key("Warning Status"), Content, System(SystemColor::Yellow)),
    spec(Key::StatusError, "status.error", Base, key("Error Status"), Content, System(SystemColor::Red)),
    spec(Key::StatusOff, "status.off", Base, key("Idle Status"), Content, System(SystemColor::TertiaryLabel)),
    spec(Key::Destructive, "destructive", Base, key("Destructive"), Content, System(SystemColor::Red)),
    spec(Key::WindowCanvas, "window.canvas", Window, key("Background"), Surface, Fallback::Key(Key::Canvas)),
    spec(Key::SidebarIcon, "sidebar.icon", Sidebar, key("Icons"), Content, Accent),
    spec(Key::SidebarCount, "sidebar.count", Sidebar, key("Counts"), Content, Fallback::Key(Key::TextTertiary)),
    spec(
        Key::ToolbarPill,
        "toolbar.pill",
        Toolbar,
        key("Thumbnail Well"),
        Surface,
        System(SystemColor::QuaternaryLabel),
    ),
    spec(
        Key::ToolbarPillHover,
        "toolbar.pill.hover",
        Toolbar,
        key("Hover"),
        Surface,
        System(SystemColor::QuaternaryLabel),
    ),
    spec(Key::ToolbarSeparator, "toolbar.separator", Toolbar, key("Separator"), Line, Fallback::Key(Key::Hairline)),
    spec(Key::GridTile, "grid.tile", Grid, key("Tile"), Surface, Fallback::Key(Key::SurfaceRaised)),
    spec(Key::GridTileBorder, "grid.tile.border", Grid, key("Tile Border"), Line, Fallback::Key(Key::Hairline)),
    spec(Key::GridRing, "grid.ring", Grid, key("Selection Ring"), Content, Fallback::Key(Key::Selection)),
    spec(Key::GridTitle, "grid.title", Grid, key("Title"), Content, Fallback::Key(Key::Text)),
    spec(
        Key::GridTitleSelected,
        "grid.title.selected",
        Grid,
        key("Selected Title"),
        Content,
        Fallback::Key(Key::Selection),
    ),
    spec(Key::GridMeta, "grid.meta", Grid, key("Details"), Content, Fallback::Key(Key::TextTertiary)),
    spec(Key::GridHighlight, "grid.highlight", Grid, key("Row Highlight"), Surface, Fallback::Key(Key::SelectionSoft)),
    spec(Key::GridProgress, "grid.progress", Grid, key("Progress"), Content, Fallback::Key(Key::Selection)),
    spec(Key::GridShade, "grid.shade", Grid, key("Top Shade"), Surface, Fallback::Key(Key::Canvas)),
    spec(Key::GridShadow, "grid.shadow", Grid, key("Tile Shadow"), Shadow, Fallback::Key(Key::Shadow)),
    spec(Key::GridEmpty, "grid.empty", Grid, key("Empty State"), Content, Fallback::Key(Key::TextTertiary)),
    spec(Key::ChipFill, "chip.fill", Chips, key("Chip"), Surface, Fixed(SCRIM, SCRIM)),
    spec(Key::ChipText, "chip.text", Chips, key("Chip Text"), Content, Fixed(Rgba::WHITE, Rgba::WHITE)),
    spec(Key::ChipActive, "chip.active", Chips, key("Active Chip"), Surface, Fallback::Key(Key::Selection)),
    spec(Key::PillFill, "pill.fill", Downloads, key("Background"), Surface, Fallback::Key(Key::SurfaceRaised)),
    spec(Key::PillBar, "pill.bar", Downloads, key("Progress"), Content, Fallback::Key(Key::Selection)),
    spec(Key::PillTrack, "pill.track", Downloads, key("Track"), Line, System(SystemColor::QuaternaryLabel)),
    spec(
        Key::InspectorBackdrop,
        "inspector.backdrop",
        Inspector,
        key("Background"),
        Surface,
        Fallback::Key(Key::Chrome),
    ),
    spec(
        Key::InspectorPreviewBorder,
        "inspector.preview.border",
        Inspector,
        key("Preview Border"),
        Line,
        Fallback::Key(Key::Hairline),
    ),
    spec(Key::InspectorSlider, "inspector.slider", Inspector, key("Sliders"), Content, Fallback::Key(Key::Selection)),
    spec(
        Key::InspectorSegment,
        "inspector.segment",
        Inspector,
        key("Segmented Control"),
        Content,
        Fallback::Key(Key::Selection),
    ),
    spec(Key::SettingsCard, "settings.card", Settings, key("Card"), Surface, Fallback::Key(Key::Surface)),
    spec(Key::SettingsSeparator, "settings.separator", Settings, key("Separator"), Line, Fallback::Key(Key::Hairline)),
    spec(
        Key::SettingsRecorder,
        "settings.recorder",
        Settings,
        key("Shortcut Field"),
        Surface,
        Fallback::Key(Key::Surface),
    ),
    spec(
        Key::SettingsRecorderActive,
        "settings.recorder.active",
        Settings,
        key("Recording"),
        Content,
        Fallback::Key(Key::Selection),
    ),
    spec(Key::SettingsSlider, "settings.slider", Settings, key("Sliders"), Content, Fallback::Key(Key::Selection)),
    spec(
        Key::PickerSelected,
        "picker.selected",
        Picker,
        key("Selected Cell"),
        Surface,
        Fallback::Key(Key::SelectionSoft),
    ),
    spec(Key::PickerDivider, "picker.divider", Picker, key("Divider"), Line, Fallback::Key(Key::Hairline)),
    spec(Key::FieldFill, "field.fill", Fields, key("Field"), Surface, Fallback::Key(Key::Surface)),
    spec(Key::FieldText, "field.text", Fields, key("Field Text"), Content, Fallback::Key(Key::TextSecondary)),
];

impl Key {
    pub const ALL: [Self; 54] = [
        Self::Canvas,
        Self::Chrome,
        Self::Surface,
        Self::SurfaceRaised,
        Self::Text,
        Self::TextSecondary,
        Self::TextTertiary,
        Self::Hairline,
        Self::Selection,
        Self::SelectionSoft,
        Self::OnAccent,
        Self::Shadow,
        Self::GlassTint,
        Self::StatusOk,
        Self::StatusWarn,
        Self::StatusError,
        Self::StatusOff,
        Self::Destructive,
        Self::WindowCanvas,
        Self::SidebarIcon,
        Self::SidebarCount,
        Self::ToolbarPill,
        Self::ToolbarPillHover,
        Self::ToolbarSeparator,
        Self::GridTile,
        Self::GridTileBorder,
        Self::GridRing,
        Self::GridTitle,
        Self::GridTitleSelected,
        Self::GridMeta,
        Self::GridHighlight,
        Self::GridProgress,
        Self::GridShade,
        Self::GridShadow,
        Self::GridEmpty,
        Self::ChipFill,
        Self::ChipText,
        Self::ChipActive,
        Self::PillFill,
        Self::PillBar,
        Self::PillTrack,
        Self::InspectorBackdrop,
        Self::InspectorPreviewBorder,
        Self::InspectorSlider,
        Self::InspectorSegment,
        Self::SettingsCard,
        Self::SettingsSeparator,
        Self::SettingsRecorder,
        Self::SettingsRecorderActive,
        Self::SettingsSlider,
        Self::PickerSelected,
        Self::PickerDivider,
        Self::FieldFill,
        Self::FieldText,
    ];

    fn spec(self) -> &'static Spec {
        let spec = &SPECS[self as usize];
        debug_assert_eq!(spec.key, self);
        return spec;
    }

    pub fn id(self) -> &'static str {
        return self.spec().id;
    }

    pub fn title(self) -> &'static str {
        return self.spec().title;
    }

    pub fn group(self) -> Group {
        return self.spec().group;
    }

    pub fn role(self) -> Role {
        return self.spec().role;
    }

    pub fn fallback(self) -> Fallback {
        return self.spec().fallback;
    }

    pub fn from_id(id: &str) -> Option<Self> {
        return Self::ALL.into_iter().find(|key| key.id() == id);
    }
}

pub const ACCENT: &str = "accent";
pub const TINT: &str = "tint";
pub const TINT_STRENGTH: &str = "tint_strength";
pub const GENERAL: [(&str, &str); 3] =
    [(ACCENT, key("Accent")), (TINT, key("Tint")), (TINT_STRENGTH, key("Tint Strength"))];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn registry_is_in_key_order_with_unique_ids() {
        assert_eq!(SPECS.len(), Key::ALL.len());
        for (spec, key) in SPECS.iter().zip(Key::ALL) {
            assert_eq!(spec.key, key);
            assert_eq!(key.spec().key, key);
        }
        let ids: BTreeSet<&str> = Key::ALL.iter().map(|key| key.id()).collect();
        assert_eq!(ids.len(), Key::ALL.len());
        assert!(GENERAL.iter().all(|(id, _)| !ids.contains(id)));
        for key in Key::ALL {
            assert_eq!(Key::from_id(key.id()), Some(key));
            assert!(key.id().chars().all(|character| character.is_ascii_lowercase() || matches!(character, '.' | '_')));
        }
        assert!(Key::from_id("nope").is_none());
    }

    #[test]
    fn fallbacks_never_loop_and_parts_lean_on_base_colors() {
        for key in Key::ALL {
            let mut seen = BTreeSet::from([key]);
            let mut current = key;
            while let Fallback::Key(next) = current.fallback() {
                assert!(seen.insert(next), "{key:?} loops");
                current = next;
            }
            if key.group() != Group::Base {
                let leans = matches!(key.fallback(), Fallback::Key(next) if next.group() == Group::Base);
                let own = matches!(key.fallback(), Fallback::System(_) | Fallback::Fixed(..) | Fallback::Accent);
                assert!(leans || own, "{key:?}");
            }
        }
    }

    #[test]
    fn every_group_has_keys() {
        for group in Group::ALL {
            assert!(group.keys().next().is_some(), "{group:?}");
        }
        assert_eq!(Group::ALL.iter().map(|group| group.keys().count()).sum::<usize>(), Key::ALL.len());
    }
}
