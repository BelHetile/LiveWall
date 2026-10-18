use crate::i18n::key;
use crate::style::color::Rgba;

const NEUTRAL_CHROMA: f64 = 0.04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Gray,
}

impl Tag {
    pub const ALL: [Self; 8] =
        [Self::Red, Self::Orange, Self::Yellow, Self::Green, Self::Blue, Self::Purple, Self::Pink, Self::Gray];

    pub fn id(self) -> &'static str {
        return match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Pink => "pink",
            Self::Gray => "gray",
        };
    }

    pub fn title(self) -> &'static str {
        return match self {
            Self::Red => key("Red"),
            Self::Orange => key("Orange"),
            Self::Yellow => key("Yellow"),
            Self::Green => key("Green"),
            Self::Blue => key("Blue"),
            Self::Purple => key("Purple"),
            Self::Pink => key("Pink"),
            Self::Gray => key("Gray"),
        };
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        return Self::ALL.into_iter().find(|tag| tag.id().eq_ignore_ascii_case(text));
    }

    pub fn reference(self) -> Rgba {
        return Rgba::hex(match self {
            Self::Red => 0xFF_383C,
            Self::Orange => 0xFF_8D28,
            Self::Yellow => 0xFF_CC00,
            Self::Green => 0x34_C759,
            Self::Blue => 0x00_88FF,
            Self::Purple => 0xCB_30E0,
            Self::Pink => 0xFF_2D55,
            Self::Gray => 0x8E_8E93,
        });
    }

    pub fn nearest(color: Rgba) -> Self {
        let lch = color.oklch();
        if lch.c < NEUTRAL_CHROMA {
            return Self::Gray;
        }
        let gap = |tag: &Self| {
            let turn = (tag.reference().oklch().h - lch.h).abs();
            return turn.min(360.0 - turn);
        };
        let chromatic = Self::ALL.into_iter().filter(|tag| *tag != Self::Gray);
        return chromatic.min_by(|a, b| gap(a).total_cmp(&gap(b))).unwrap_or(Self::Gray);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_parse_case_insensitively() {
        assert_eq!(Tag::parse("Red"), Some(Tag::Red));
        assert_eq!(Tag::parse(" gray "), Some(Tag::Gray));
        assert!(Tag::parse("grey").is_none());
        assert!(Tag::parse("#FF0000").is_none());
        for tag in Tag::ALL {
            assert_eq!(Tag::parse(tag.id()), Some(tag));
        }
    }

    #[test]
    fn old_folder_presets_map_to_the_nearest_tag() {
        let expected = [
            (0xD9_785A, Tag::Red),
            (0xD9_A441, Tag::Yellow),
            (0x7F_A77F, Tag::Green),
            (0x5E_8C6A, Tag::Green),
            (0x6A_8CC4, Tag::Blue),
            (0x9C_88C9, Tag::Purple),
            (0xC9_7C8C, Tag::Pink),
            (0x9C_7055, Tag::Orange),
        ];
        for (value, tag) in expected {
            assert_eq!(Tag::nearest(Rgba::hex(value)), tag, "{value:06X}");
        }
        assert_eq!(Tag::nearest(Rgba::hex(0x80_8080)), Tag::Gray);
    }
}
