#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

fn to_linear(channel: f64) -> f64 {
    return if channel <= 0.040_45 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) };
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte(unit: f64) -> u8 {
    return (unit.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn unit(byte: u8) -> f64 {
    return f64::from(byte) / 255.0;
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        return Self { r, g, b, a };
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        return Self::new(r, g, b, 255);
    }

    #[allow(clippy::cast_possible_truncation)]
    pub const fn hex(value: u32) -> Self {
        return Self::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
    }

    pub fn unit(r: f64, g: f64, b: f64, a: f64) -> Self {
        return Self::new(byte(r), byte(g), byte(b), byte(a));
    }

    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim();
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if !matches!(hex.len(), 6 | 8) || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(hex, 16).ok()?;
        return Some(if hex.len() == 6 { Self::hex(value) } else { Self::hex(value >> 8).with_byte(value) });
    }

    #[allow(clippy::cast_possible_truncation)]
    const fn with_byte(self, value: u32) -> Self {
        return Self { a: value as u8, ..self };
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            return format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b);
        }
        return format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a);
    }

    pub fn components(self) -> [f64; 4] {
        return [unit(self.r), unit(self.g), unit(self.b), unit(self.a)];
    }

    pub fn opaque(self) -> Self {
        return Self { a: 255, ..self };
    }

    pub fn oklab(self) -> Oklab {
        let [red, green, blue, _] = self.components().map(to_linear);
        let long = (0.412_221_470_8 * red + 0.536_332_536_3 * green + 0.051_445_992_9 * blue).cbrt();
        let medium = (0.211_903_498_2 * red + 0.680_699_545_1 * green + 0.107_396_956_6 * blue).cbrt();
        let short = (0.088_302_461_9 * red + 0.281_718_837_6 * green + 0.629_978_700_5 * blue).cbrt();
        return Oklab {
            l: 0.210_454_255_3 * long + 0.793_617_785_0 * medium - 0.004_072_046_8 * short,
            a: 1.977_998_495_1 * long - 2.428_592_205_0 * medium + 0.450_593_709_9 * short,
            b: 0.025_904_037_1 * long + 0.782_771_766_2 * medium - 0.808_675_766_0 * short,
        };
    }

    pub fn oklch(self) -> Oklch {
        return self.oklab().lch();
    }
}

impl Oklab {
    pub fn lch(self) -> Oklch {
        return Oklch { l: self.l, c: self.a.hypot(self.b), h: self.b.atan2(self.a).to_degrees().rem_euclid(360.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        return (a - b).abs() <= tolerance;
    }

    #[test]
    fn hex_parses_and_formats() {
        assert_eq!(Rgba::parse("#1a2B3c"), Some(Rgba::rgb(0x1A, 0x2B, 0x3C)));
        assert_eq!(Rgba::parse(" 1A2B3C "), Some(Rgba::rgb(0x1A, 0x2B, 0x3C)));
        assert_eq!(Rgba::parse("#1A2B3C80"), Some(Rgba::new(0x1A, 0x2B, 0x3C, 0x80)));
        assert_eq!(Rgba::parse("#1A2B3C").map(Rgba::to_hex).as_deref(), Some("#1A2B3C"));
        assert_eq!(Rgba::new(1, 2, 3, 4).to_hex(), "#01020304");
        for bad in ["", "#12345", "#1234567", "#GGGGGG", "red", "#123456789"] {
            assert!(Rgba::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn oklab_matches_reference_values() {
        let white = Rgba::hex(0xFF_FFFF).oklab();
        assert!(close(white.l, 1.0, 1e-3) && close(white.a, 0.0, 1e-3) && close(white.b, 0.0, 1e-3));
        let blue = Rgba::hex(0x00_7AFF).oklch();
        assert!(close(blue.l, 0.603, 2e-3) && close(blue.c, 0.218, 2e-3) && close(blue.h, 257.4, 0.5), "{blue:?}");
    }
}
