use std::fmt;

const DARK_LIFT: f64 = 0.08;
const DARK_LIFT_RANGE: f64 = 0.07;
const DARK_CHROMA: f64 = 0.9;
const DARK_FLOOR: f64 = 0.22;
const DARK_CEILING: f64 = 0.98;
const NEUTRAL_CHROMA: f64 = 0.03;
const MAX_LIGHTNESS: f64 = 0.98;
const CONTRAST_STEP: f64 = 0.04;
const CONTRAST_GOAL: f64 = 7.0;
const LABEL_ALPHA: f64 = 0.85;
const LEGIBLE: f64 = 4.5;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Surface,
    Content,
    Line,
    Shadow,
}

fn to_linear(channel: f64) -> f64 {
    return if channel <= 0.040_45 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) };
}

fn to_gamma(channel: f64) -> f64 {
    return if channel <= 0.003_130_8 { channel * 12.92 } else { 1.055 * channel.powf(1.0 / 2.4) - 0.055 };
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte(unit: f64) -> u8 {
    return (unit.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn unit(byte: u8) -> f64 {
    return f64::from(byte) / 255.0;
}

fn in_gamut(channels: [f64; 3]) -> bool {
    return channels.iter().all(|channel| (-1e-4..=1.0 + 1e-4).contains(channel));
}

impl Rgba {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const CLEAR: Self = Self::new(0, 0, 0, 0);

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

    pub fn alpha(self) -> f64 {
        return unit(self.a);
    }

    pub fn with_alpha(self, alpha: f64) -> Self {
        return Self { a: byte(alpha), ..self };
    }

    pub fn scale_alpha(self, factor: f64) -> Self {
        return self.with_alpha(self.alpha() * factor);
    }

    pub fn opaque(self) -> Self {
        return Self { a: 255, ..self };
    }

    pub fn luminance(self) -> f64 {
        let [r, g, b, _] = self.components();
        return 0.2126 * to_linear(r) + 0.7152 * to_linear(g) + 0.0722 * to_linear(b);
    }

    pub fn over(self, background: Self) -> Self {
        let [r, g, b, a] = self.components();
        let [br, bg, bb, ba] = background.components();
        let out = a + ba * (1.0 - a);
        if out <= 0.0 {
            return Self::CLEAR;
        }
        let blend = |top: f64, bottom: f64| (top * a + bottom * ba * (1.0 - a)) / out;
        return Self::unit(blend(r, br), blend(g, bg), blend(b, bb), out);
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

    pub fn mix(self, other: Self, amount: f64) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let (from, to) = (self.oklab(), other.oklab());
        let lerp = |a: f64, b: f64| a + (b - a) * amount;
        let mixed = Oklab { l: lerp(from.l, to.l), a: lerp(from.a, to.a), b: lerp(from.b, to.b) };
        return mixed.rgba(lerp(self.alpha(), other.alpha()));
    }

    pub fn derive_dark(self) -> Self {
        let lch = self.oklch();
        if lch.c < NEUTRAL_CHROMA {
            let l = DARK_FLOOR + (1.0 - lch.l) * (DARK_CEILING - DARK_FLOOR);
            return Oklch { l, ..lch }.rgba(self.alpha());
        }
        let c = lch.c * DARK_CHROMA;
        let target = (lch.l + DARK_LIFT + DARK_LIFT_RANGE * (1.0 - lch.l)).min(MAX_LIGHTNESS);
        let fits = |l: f64| in_gamut(Oklch { l, c, h: lch.h }.lab(c).linear());
        if fits(target) || !fits(lch.l) {
            return Oklch { l: target, c, h: lch.h }.rgba(self.alpha());
        }
        let (mut low, mut high) = (lch.l, target);
        for _ in 0..32 {
            let middle = f64::midpoint(low, high);
            if fits(middle) {
                low = middle;
            } else {
                high = middle;
            }
        }
        return Oklch { l: low, c, h: lch.h }.rgba(self.alpha());
    }

    pub fn derive_contrast(self, background: Self, role: Role) -> Self {
        return match role {
            Role::Surface | Role::Shadow => self.with_alpha((self.alpha() * 1.5).min(1.0)),
            Role::Line => self.with_alpha((self.alpha() * 2.0).min(1.0)).push_from(background, 0.1),
            Role::Content => self.opaque().separate(background),
        };
    }

    fn push_from(self, background: Self, amount: f64) -> Self {
        let lch = self.oklch();
        let away = if lch.l >= background.oklch().l { amount } else { -amount };
        return Oklch { l: (lch.l + away).clamp(0.0, 1.0), ..lch }.rgba(self.alpha());
    }

    fn separate(self, background: Self) -> Self {
        let mut color = self;
        for _ in 0..24 {
            if contrast(color, background) >= CONTRAST_GOAL {
                break;
            }
            let next = color.push_from(background, CONTRAST_STEP);
            if next == color {
                break;
            }
            color = next;
        }
        return color;
    }

    pub fn on(self) -> Self {
        let fill = self.opaque();
        let black = Self::BLACK.with_alpha(LABEL_ALPHA);
        let white_contrast = contrast(Self::WHITE, fill);
        if white_contrast >= LEGIBLE || white_contrast >= contrast(black, fill) {
            return Self::WHITE;
        }
        return black;
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(&self.to_hex());
    }
}

pub fn contrast(foreground: Rgba, background: Rgba) -> f64 {
    let background = background.opaque();
    let foreground = foreground.over(background);
    let (a, b) = (foreground.luminance(), background.luminance());
    return (a.max(b) + 0.05) / (a.min(b) + 0.05);
}

impl Oklab {
    pub fn lch(self) -> Oklch {
        return Oklch { l: self.l, c: self.a.hypot(self.b), h: self.b.atan2(self.a).to_degrees().rem_euclid(360.0) };
    }

    fn linear(self) -> [f64; 3] {
        let long = (self.l + 0.396_337_777_4 * self.a + 0.215_803_757_3 * self.b).powi(3);
        let medium = (self.l - 0.105_561_345_8 * self.a - 0.063_854_172_8 * self.b).powi(3);
        let short = (self.l - 0.089_484_177_5 * self.a - 1.291_485_548_0 * self.b).powi(3);
        return [
            4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
            -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
            -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short,
        ];
    }

    pub fn rgba(self, alpha: f64) -> Rgba {
        let [r, g, b] = self.linear();
        return Rgba::unit(to_gamma(r.max(0.0)), to_gamma(g.max(0.0)), to_gamma(b.max(0.0)), alpha);
    }
}

impl Oklch {
    fn lab(self, chroma: f64) -> Oklab {
        let angle = self.h.to_radians();
        return Oklab { l: self.l, a: chroma * angle.cos(), b: chroma * angle.sin() };
    }

    pub fn rgba(self, alpha: f64) -> Rgba {
        let l = self.l.clamp(0.0, 1.0);
        let fitted = Self { l, ..self };
        if in_gamut(fitted.lab(self.c).linear()) {
            return fitted.lab(self.c).rgba(alpha);
        }
        let (mut low, mut high) = (0.0, self.c);
        for _ in 0..32 {
            let middle = f64::midpoint(low, high);
            if in_gamut(fitted.lab(middle).linear()) {
                low = middle;
            } else {
                high = middle;
            }
        }
        return fitted.lab(low).rgba(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        return (a - b).abs() <= tolerance;
    }

    fn distance(a: Rgba, b: Rgba) -> f64 {
        let (a, b) = (a.oklab(), b.oklab());
        return ((a.l - b.l).powi(2) + (a.a - b.a).powi(2) + (a.b - b.b).powi(2)).sqrt();
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
    fn oklab_round_trips_every_channel() {
        for value in [0x00_0000, 0xFF_FFFF, 0x00_7AFF, 0xFF_3B30, 0x34_C759, 0x80_8080, 0x12_3456, 0xFE_DCBA] {
            let color = Rgba::hex(value);
            assert_eq!(color.oklab().rgba(1.0), color, "{color}");
            assert_eq!(color.oklch().rgba(1.0), color, "{color}");
        }
    }

    #[test]
    fn oklab_matches_reference_values() {
        let white = Rgba::WHITE.oklab();
        assert!(close(white.l, 1.0, 1e-3) && close(white.a, 0.0, 1e-3) && close(white.b, 0.0, 1e-3));
        let blue = Rgba::hex(0x00_7AFF).oklch();
        assert!(close(blue.l, 0.603, 2e-3) && close(blue.c, 0.218, 2e-3) && close(blue.h, 257.4, 0.5), "{blue:?}");
    }

    #[test]
    fn out_of_gamut_chroma_is_reduced_not_clipped() {
        let color = Oklch { l: 0.7, c: 0.4, h: 150.0 }.rgba(1.0);
        let back = color.oklch();
        assert!(close(back.h, 150.0, 2.0), "hue drifted: {back:?}");
        assert!(back.c < 0.4);
    }

    #[test]
    fn contrast_matches_wcag_references() {
        assert!(close(contrast(Rgba::BLACK, Rgba::WHITE), 21.0, 1e-9));
        assert!(close(contrast(Rgba::WHITE, Rgba::WHITE), 1.0, 1e-9));
        assert!(close(contrast(Rgba::hex(0x00_7AFF), Rgba::WHITE), 4.02, 0.02));
        assert!(close(contrast(Rgba::hex(0x76_7676), Rgba::WHITE), 4.54, 0.02));
    }

    #[test]
    fn translucent_foregrounds_are_composited_first() {
        let half = Rgba::BLACK.with_alpha(0.5);
        let mid = contrast(half, Rgba::WHITE);
        assert!(mid > 1.0 && mid < contrast(Rgba::BLACK, Rgba::WHITE));
        assert_eq!(half.over(Rgba::WHITE), Rgba::rgb(127, 127, 127));
        assert_eq!(Rgba::CLEAR.over(Rgba::CLEAR), Rgba::CLEAR);
    }

    #[test]
    fn mixing_happens_in_oklab_and_keeps_endpoints() {
        let a = Rgba::hex(0x00_7AFF);
        let b = Rgba::hex(0xFF_3B30);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        let middle = a.mix(b, 0.5).oklab();
        let expected = f64::midpoint(a.oklab().l, b.oklab().l);
        assert!(close(middle.l, expected, 5e-3));
        assert!(close(Rgba::BLACK.with_alpha(0.0).mix(Rgba::BLACK, 0.5).alpha(), 0.5, 0.01));
    }

    #[test]
    fn derived_dark_of_apple_system_colors_lands_near_apple_dark() {
        let pairs = [(0x00_7AFF, 0x0A_84FF), (0xFF_3B30, 0xFF_453A), (0x28_CD41, 0x32_D74B), (0xAF_52DE, 0xBF_5AF2)];
        for (light, dark) in pairs {
            let (light, dark) = (Rgba::hex(light), Rgba::hex(dark));
            let derived = light.derive_dark();
            assert!(derived.oklch().l > light.oklch().l, "{light} -> {derived}");
            let hue = (derived.oklch().h - light.oklch().h).abs();
            assert!(hue.min(360.0 - hue) < 6.0, "{light} hue drifted to {derived}");
            assert!(distance(derived, dark) < 0.12, "{light} -> {derived}, apple {dark}");
        }
    }

    #[test]
    fn derived_dark_inverts_neutrals() {
        let paper = Rgba::hex(0xF2_F2F4).derive_dark();
        assert!(paper.oklch().l < 0.3, "{paper}");
        let ink = Rgba::hex(0x1A_1A1A).derive_dark();
        assert!(ink.oklch().l > 0.8, "{ink}");
        assert_eq!(Rgba::BLACK.with_alpha(0.3).derive_dark().a, Rgba::BLACK.with_alpha(0.3).a);
    }

    #[test]
    fn high_contrast_separates_content_from_its_background() {
        let canvas = Rgba::hex(0xE6_E6E6);
        let accent = Rgba::hex(0x5F_A8DC);
        let pushed = accent.derive_contrast(canvas, Role::Content);
        assert!(contrast(pushed, canvas) > contrast(accent, canvas));
        assert!(contrast(pushed, canvas) >= CONTRAST_GOAL);
        let line = Rgba::BLACK.with_alpha(0.1).derive_contrast(canvas, Role::Line);
        assert!(close(line.alpha(), 0.2, 0.01));
        let fill = Rgba::BLACK.with_alpha(0.4).derive_contrast(canvas, Role::Surface);
        assert!(close(fill.alpha(), 0.6, 0.01));
        let dark = Rgba::hex(0x28_2828);
        let lifted = Rgba::hex(0x6C_5B9E).derive_contrast(dark, Role::Content);
        assert!(lifted.oklch().l > Rgba::hex(0x6C_5B9E).oklch().l);
    }

    #[test]
    fn text_on_a_fill_is_legible() {
        assert_eq!(Rgba::hex(0x00_60DF).on(), Rgba::WHITE);
        assert_eq!(Rgba::hex(0x00_7AFF).on(), Rgba::BLACK.with_alpha(LABEL_ALPHA));
        assert_eq!(Rgba::hex(0x2B_6BA4).on(), Rgba::WHITE);
        assert_eq!(Rgba::hex(0xFF_CC00).on(), Rgba::BLACK.with_alpha(LABEL_ALPHA));
        assert_eq!(Rgba::WHITE.on(), Rgba::BLACK.with_alpha(LABEL_ALPHA));
        for value in [0x67_676C, 0x25_7554, 0x6C_5B9E, 0x9F_5237, 0x98_989D, 0x4F_B98A, 0xD4_B45F] {
            let fill = Rgba::hex(value);
            assert!(contrast(fill.on(), fill) >= LEGIBLE, "{fill}");
        }
    }
}
