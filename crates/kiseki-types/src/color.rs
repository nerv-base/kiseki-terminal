//! Color primitives: sRGB hex parsing, OKLCH conversion, perceptual mixing and
//! WCAG 2.1 relative luminance / contrast ratio math.
//!
//! Everything here is a pure function so themes can be validated and derived
//! deterministically without touching the host system.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{KisekiError, Result};

/// An opaque 8-bit-per-channel sRGB color.
///
/// Serialization uses the canonical uppercase `#RRGGBB` form required by the
/// theme schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

/// A color in the OKLCH perceptual color space.
///
/// `l` is lightness in `0.0..=1.0`, `c` chroma (`>= 0.0`) and `h` hue in
/// degrees `0.0..360.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

impl Color {
    pub const BLACK: Self = Self::from_rgb(0, 0, 0);
    pub const WHITE: Self = Self::from_rgb(255, 255, 255);

    /// Builds a color from raw 8-bit channels.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses a strict `#RRGGBB` string (case-insensitive hex digits).
    ///
    /// Anything else - shorthand `#RGB`, alpha forms, trailing whitespace,
    /// control characters or injection payloads - is rejected.
    pub fn parse_hex(raw: &str) -> Result<Self> {
        let bytes = raw.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' {
            return Err(KisekiError::invalid_color(raw.to_string()));
        }
        let mut channels = [0u8; 3];
        for (index, slot) in channels.iter_mut().enumerate() {
            let hi = hex_value(bytes[1 + index * 2]).ok_or_else(|| KisekiError::invalid_color(raw.to_string()))?;
            let lo = hex_value(bytes[2 + index * 2]).ok_or_else(|| KisekiError::invalid_color(raw.to_string()))?;
            *slot = (hi << 4) | lo;
        }
        Ok(Self::from_rgb(channels[0], channels[1], channels[2]))
    }

    pub const fn r(&self) -> u8 {
        self.r
    }

    pub const fn g(&self) -> u8 {
        self.g
    }

    pub const fn b(&self) -> u8 {
        self.b
    }

    /// Returns the three channels as a tuple (handy for TUI/`ratatui` swatches).
    pub const fn rgb(self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }

    /// Renders the canonical uppercase `#RRGGBB` representation.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Normalized `(r, g, b)` channels in `0.0..=1.0`, as consumed by shaders.
    pub fn to_unit_rgb(self) -> (f64, f64, f64) {
        (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
        )
    }

    /// Converts the color to OKLCH.
    pub fn to_oklch(self) -> Oklch {
        let (r, g, b) = self.to_unit_rgb();
        let to_linear = |c: f64| {
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (lr, lg, lb) = (to_linear(r), to_linear(g), to_linear(b));

        let l = 0.412_221_470_8 * lr + 0.536_332_536_3 * lg + 0.051_445_992_9 * lb;
        let m = 0.211_903_498_2 * lr + 0.680_699_545_1 * lg + 0.107_396_956_6 * lb;
        let s = 0.088_302_461_9 * lr + 0.281_718_837_6 * lg + 0.629_978_700_5 * lb;

        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        let lightness = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
        let a = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
        let bb = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;

        let chroma = (a * a + bb * bb).sqrt();
        let mut hue = bb.atan2(a).to_degrees();
        if hue < 0.0 {
            hue += 360.0;
        }

        Oklch {
            l: lightness,
            c: chroma,
            h: hue,
        }
    }

    /// WCAG 2.1 relative luminance in `0.0..=1.0`.
    pub fn relative_luminance(self) -> f64 {
        let (r, g, b) = self.to_unit_rgb();
        let channel = |c: f64| {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    /// WCAG 2.1 contrast ratio against `other`, in `1.0..=21.0`.
    pub fn contrast_ratio(self, other: Color) -> f64 {
        let a = self.relative_luminance();
        let b = other.relative_luminance();
        let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Perceptual mix in OKLCH. `ratio` of `0.0` yields `self`, `1.0` yields `other`.
    ///
    /// Hue interpolation takes the shortest path around the wheel and chroma is
    /// scaled by the result's lightness so dark blends do not turn neon.
    pub fn mix_oklch(self, other: Color, ratio: f64) -> Color {
        let ratio = ratio.clamp(0.0, 1.0);
        let from = self.to_oklch();
        let to = other.to_oklch();

        let mut hue_delta = to.h - from.h;
        if hue_delta > 180.0 {
            hue_delta -= 360.0;
        } else if hue_delta < -180.0 {
            hue_delta += 360.0;
        }
        let mut hue = from.h + hue_delta * ratio;
        if hue < 0.0 {
            hue += 360.0;
        }
        if hue >= 360.0 {
            hue -= 360.0;
        }

        let mixed = Oklch {
            l: from.l + (to.l - from.l) * ratio,
            c: from.c + (to.c - from.c) * ratio,
            h: hue,
        };
        mixed.to_color()
    }

    /// Adjusts only the OKLCH lightness axis, preserving chroma and hue.
    ///
    /// The result is chroma-clipped back into the sRGB gamut.
    pub fn with_oklch_lightness(self, lightness: f64) -> Color {
        let mut oklch = self.to_oklch();
        oklch.l = lightness.clamp(0.0, 1.0);
        oklch.to_color()
    }

    /// Raises lightness (never lowers it) until `against` reaches `target`.
    ///
    /// Returns the original color when the ratio already satisfies `target`.
    /// Because the search runs on the OKLCH lightness axis the hue identity of
    /// the accent color survives accessibility adjustment.
    pub fn boost_contrast_to(self, against: Color, target: f64) -> Color {
        if self.contrast_ratio(against) >= target {
            return self;
        }
        let base = self.to_oklch();
        // Decide the direction from the contrast the endpoint can actually
        // reach, not from a luminance midpoint: a mid-grey background
        // (#AAAAAA) is dark enough to look "dark" while black still produces
        // far more contrast than white.
        let brighten = Color::WHITE.contrast_ratio(against) >= Color::BLACK.contrast_ratio(against);
        let mut low = if brighten { base.l } else { 0.0 };
        let mut high = if brighten { 1.0 } else { base.l };

        let mut best = self;
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            let candidate = Oklch { l: mid, ..base }.to_color();
            if candidate.contrast_ratio(against) >= target {
                best = candidate;
                if brighten {
                    high = mid;
                } else {
                    low = mid;
                }
            } else if brighten {
                low = mid;
            } else {
                high = mid;
            }
        }
        best
    }
}

impl Oklch {
    /// Converts back to sRGB, clipping out-of-gamut chroma.
    pub fn to_color(self) -> Color {
        let hue_rad = self.h.to_radians();
        let a = self.c * hue_rad.cos();
        let b = self.c * hue_rad.sin();

        let l_ = self.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
        let m_ = self.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
        let s_ = self.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;

        let l3 = l_ * l_ * l_;
        let m3 = m_ * m_ * m_;
        let s3 = s_ * s_ * s_;

        let lr = 4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3;
        let lg = -1.268_438_004_6 * l3 + 2.609_757_401_1 * m3 - 0.341_319_396_5 * s3;
        let lb = -0.004_196_086_3 * l3 - 0.703_418_614_7 * m3 + 1.707_614_701_0 * s3;

        let encode = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            let srgb = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
        };

        Color::from_rgb(encode(lr), encode(lg), encode(lb))
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl std::str::FromStr for Color {
    type Err = KisekiError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse_hex(s)
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse_hex(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_hex() {
        let color = Color::parse_hex("#f06a22").unwrap();
        assert_eq!((color.r(), color.g(), color.b()), (240, 106, 34));
        assert_eq!(color.to_hex(), "#F06A22");
    }

    #[test]
    fn rejects_malformed_hex() {
        for bad in ["", "#", "#FFF", "F06A22", "#F06A2", "#F06A222", "#GGGGGG", "#F06A22\n", " #F06A22"] {
            assert!(Color::parse_hex(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn oklch_roundtrip_is_stable() {
        for hex in ["#0B090A", "#ECE4DA", "#F06A22", "#62C6F2", "#3FE08A"] {
            let color = Color::parse_hex(hex).unwrap();
            let roundtrip = color.to_oklch().to_color();
            let delta = (i16::from(color.r()) - i16::from(roundtrip.r())).abs()
                + (i16::from(color.g()) - i16::from(roundtrip.g())).abs()
                + (i16::from(color.b()) - i16::from(roundtrip.b())).abs();
            assert!(delta <= 3, "{hex} roundtrip drifted by {delta}: {roundtrip}");
        }
    }

    #[test]
    fn oklch_lightness_matches_perception() {
        // Pure white must be near L=1, pure black near L=0.
        assert!((Color::WHITE.to_oklch().l - 1.0).abs() < 0.01);
        assert!(Color::BLACK.to_oklch().l.abs() < 0.01);
        // A mid gray sits near L=0.6 in OKLab, unlike sRGB's 0.5.
        let gray = Color::from_rgb(128, 128, 128).to_oklch();
        assert!((gray.l - 0.6).abs() < 0.03, "got {}", gray.l);
        assert!(gray.c < 0.01, "grays must be achromatic");
    }

    #[test]
    fn contrast_ratio_matches_wcag_reference() {
        let black = Color::BLACK;
        let white = Color::WHITE;
        assert!((black.contrast_ratio(white) - 21.0).abs() < 0.01);
        assert!((black.contrast_ratio(black) - 1.0).abs() < f64::EPSILON);
        // #767676 on white is the canonical 4.54:1 WCAG AA boundary.
        let gray = Color::parse_hex("#767676").unwrap();
        assert!((gray.contrast_ratio(white) - 4.54).abs() < 0.05);
    }

    #[test]
    fn mixing_is_perceptual_and_bounded() {
        let base = Color::parse_hex("#0B090A").unwrap();
        let accent = Color::parse_hex("#F06A22").unwrap();
        assert_eq!(base.mix_oklch(accent, 0.0), base);
        assert_eq!(base.mix_oklch(accent, 1.0), accent);
        let half = base.mix_oklch(accent, 0.5);
        let lum = half.relative_luminance();
        assert!(lum > base.relative_luminance() && lum < accent.relative_luminance());
    }

    #[test]
    fn contrast_boost_reaches_target_and_keeps_hue() {
        let bg = Color::parse_hex("#0B090A").unwrap();
        let dim = Color::parse_hex("#55383B").unwrap();
        assert!(dim.contrast_ratio(bg) < 7.0);
        let boosted = dim.boost_contrast_to(bg, 7.0);
        assert!(boosted.contrast_ratio(bg) >= 7.0, "got {}", boosted.contrast_ratio(bg));
        let before = dim.to_oklch();
        let after = boosted.to_oklch();
        assert!((before.h - after.h).abs() < 12.0, "hue drifted {} -> {}", before.h, after.h);
    }

    #[test]
    fn contrast_boost_is_noop_when_already_passing() {
        let bg = Color::parse_hex("#0B090A").unwrap();
        let bright = Color::parse_hex("#ECE4DA").unwrap();
        assert_eq!(bright.boost_contrast_to(bg, 7.0), bright);
    }

    #[test]
    fn contrast_boost_darkens_on_light_backgrounds() {
        let bg = Color::parse_hex("#FAFAFA").unwrap();
        let pale = Color::parse_hex("#E0E0E0").unwrap();
        let boosted = pale.boost_contrast_to(bg, 7.0);
        assert!(boosted.contrast_ratio(bg) >= 7.0);
        assert!(boosted.relative_luminance() < pale.relative_luminance());
    }

    #[test]
    fn contrast_boost_picks_the_direction_from_reachable_contrast() {
        // #AAAAAA is mid-grey: `luminance < 0.5` would wrongly call it "dark"
        // and brighten the foreground, which can never reach 7:1 against it.
        let bg = Color::parse_hex("#AAAAAA").unwrap();
        assert!(Color::WHITE.contrast_ratio(bg) < Color::BLACK.contrast_ratio(bg));
        let boosted = bg.boost_contrast_to(bg, 7.0);
        assert!(
            boosted.contrast_ratio(bg) >= 7.0,
            "got {} for {boosted:?}",
            boosted.contrast_ratio(bg)
        );
        assert!(boosted.relative_luminance() < bg.relative_luminance());

        let against_white = Color::parse_hex("#AAAAAA")
            .unwrap()
            .boost_contrast_to(Color::WHITE, 7.0);
        assert!(against_white.contrast_ratio(Color::WHITE) >= 7.0);
        assert!(against_white.relative_luminance() < 0.2);
    }

    #[test]
    fn serde_uses_canonical_uppercase_hex() {
        let color: Color = serde_json::from_str("\"#f06a22\"").unwrap();
        assert_eq!(serde_json::to_string(&color).unwrap(), "\"#F06A22\"");
        assert!(serde_json::from_str::<Color>("\"#F06A2\"").is_err());
    }
}
