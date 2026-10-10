//! Цвет и преобразования цветовых пространств.
//!
//! Внутри всё считается в sRGB с компонентами 0..1, но осветление, затемнение и
//! подбор оттенков идут через OKLCH. Это важно: в HSL «осветлить на 10%» для
//! жёлтого и синего даёт визуально разный шаг, из-за чего палитра выглядит
//! грязной. OKLab строился так, чтобы равные шаги воспринимались глазом
//! одинаково, поэтому вся палитра получается ровной.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Ошибка разбора цвета из строки.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ColorParseError {
    #[error("цвет должен начинаться с '#', получено: {0:?}")]
    MissingHash(String),
    #[error("неверная длина: ожидалось 3, 4, 6 или 8 шестнадцатеричных цифр, получено {0}")]
    BadLength(usize),
    #[error("не шестнадцатеричная цифра: {0:?}")]
    BadDigit(char),
}

/// Цвет в sRGB с альфа-каналом. Компоненты в диапазоне 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// Цвет из привычных 8-битных компонентов.
    ///
    /// В отличие от [`Color::rgb`] с дробными литералами, такой цвет переживает
    /// запись в конфиг и чтение обратно без расхождения в последнем знаке.
    pub const fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
            a: 1.0,
        }
    }

    /// Разбирает `#rgb`, `#rgba`, `#rrggbb` или `#rrggbbaa`.
    pub fn from_hex(s: &str) -> Result<Self, ColorParseError> {
        let body = s
            .strip_prefix('#')
            .ok_or_else(|| ColorParseError::MissingHash(s.to_string()))?;

        if let Some(bad) = body.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(ColorParseError::BadDigit(bad));
        }

        let digits: Vec<u8> = body
            .chars()
            .map(|c| c.to_digit(16).expect("проверено выше") as u8)
            .collect();

        // Короткая форма #rgb — каждая цифра удваивается, как в CSS.
        let expand = |hi: u8, lo: u8| (hi * 16 + lo) as f64 / 255.0;
        let short = |d: u8| (d * 16 + d) as f64 / 255.0;

        match digits.len() {
            3 => Ok(Color::rgb(
                short(digits[0]),
                short(digits[1]),
                short(digits[2]),
            )),
            4 => Ok(Color::rgba(
                short(digits[0]),
                short(digits[1]),
                short(digits[2]),
                short(digits[3]),
            )),
            6 => Ok(Color::rgb(
                expand(digits[0], digits[1]),
                expand(digits[2], digits[3]),
                expand(digits[4], digits[5]),
            )),
            8 => Ok(Color::rgba(
                expand(digits[0], digits[1]),
                expand(digits[2], digits[3]),
                expand(digits[4], digits[5]),
                expand(digits[6], digits[7]),
            )),
            other => Err(ColorParseError::BadLength(other)),
        }
    }

    /// `#rrggbb`, либо `#rrggbbaa`, если цвет не полностью непрозрачный.
    pub fn to_hex(self) -> String {
        let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", q(self.r), q(self.g), q(self.b))
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                q(self.r),
                q(self.g),
                q(self.b),
                q(self.a)
            )
        }
    }

    /// Запись в виде `rgba(r, g, b, a)` — то, что понимает CSS в GTK.
    pub fn to_css_rgba(self) -> String {
        let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "rgba({}, {}, {}, {:.3})",
            q(self.r),
            q(self.g),
            q(self.b),
            self.a.clamp(0.0, 1.0)
        )
    }

    pub fn with_alpha(self, a: f64) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Смешивает два цвета: `t = 0` даёт `self`, `t = 1` даёт `other`.
    ///
    /// Смешивание идёт в OKLab, поэтому середина между синим и жёлтым —
    /// нейтральный серый, а не грязно-зелёный, как получилось бы в sRGB.
    pub fn mix(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let a = self.to_oklab();
        let b = other.to_oklab();
        let mixed = Oklab {
            l: a.l + (b.l - a.l) * t,
            a: a.a + (b.a - a.a) * t,
            b: a.b + (b.b - a.b) * t,
        };
        let mut out = mixed.to_color();
        out.a = self.a + (other.a - self.a) * t;
        out
    }

    /// Яркость по WCAG — нужна для проверки контраста.
    pub fn relative_luminance(self) -> f64 {
        0.2126 * srgb_to_linear(self.r)
            + 0.7152 * srgb_to_linear(self.g)
            + 0.0722 * srgb_to_linear(self.b)
    }

    /// Контраст по WCAG 2.1: от 1 (одинаковые) до 21 (чёрный на белом).
    ///
    /// Порог 4.5 — обычный текст, 3.0 — крупный текст и элементы интерфейса.
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let a = self.relative_luminance();
        let b = other.relative_luminance();
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Белый или чёрный — тот, что читается на этом фоне лучше.
    pub fn best_foreground(self) -> Color {
        if self.contrast_ratio(Color::WHITE) >= self.contrast_ratio(Color::BLACK) {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

    pub fn to_oklab(self) -> Oklab {
        Oklab::from_color(self)
    }

    pub fn to_oklch(self) -> Oklch {
        self.to_oklab().to_oklch()
    }

    /// Тот же оттенок и насыщенность, но заданная светлота (0..1).
    pub fn with_lightness(self, l: f64) -> Color {
        let mut lch = self.to_oklch();
        lch.l = l.clamp(0.0, 1.0);
        let mut out = lch.to_color();
        out.a = self.a;
        out
    }

    /// Тот же оттенок и светлота, но заданная насыщенность.
    pub fn with_chroma(self, c: f64) -> Color {
        let mut lch = self.to_oklch();
        lch.c = c.max(0.0);
        let mut out = lch.to_color();
        out.a = self.a;
        out
    }

    /// Светлее на `amount` единиц светлоты OKLab.
    pub fn lighten(self, amount: f64) -> Color {
        let l = self.to_oklch().l;
        self.with_lightness(l + amount)
    }

    /// Темнее на `amount` единиц светлоты OKLab.
    pub fn darken(self, amount: f64) -> Color {
        let l = self.to_oklch().l;
        self.with_lightness(l - amount)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Color::from_hex(s)
    }
}

// В конфигах цвет живёт как строка "#rrggbb" — так его правит человек.
impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Color::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// Цвет в OKLab: `l` — светлота, `a`/`b` — оси зелёный–красный и синий–жёлтый.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Oklab {
    pub fn from_color(c: Color) -> Self {
        let r = srgb_to_linear(c.r);
        let g = srgb_to_linear(c.g);
        let b = srgb_to_linear(c.b);

        let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        Oklab {
            l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
            a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
            b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
        }
    }

    /// Линейный RGB без обрезки. Значения вне 0..1 означают, что цвет не
    /// изображается в sRGB.
    fn to_linear_rgb(self) -> (f64, f64, f64) {
        let l_ = self.l + 0.3963377774 * self.a + 0.2158037573 * self.b;
        let m_ = self.l - 0.1055613458 * self.a - 0.0638541728 * self.b;
        let s_ = self.l - 0.0894841775 * self.a - 1.2914855480 * self.b;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        (
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        )
    }

    /// Умещается ли цвет в sRGB.
    fn is_in_gamut(self) -> bool {
        let (r, g, b) = self.to_linear_rgb();
        const SLACK: f64 = 1e-6;
        [r, g, b].iter().all(|c| *c >= -SLACK && *c <= 1.0 + SLACK)
    }

    pub fn to_color(self) -> Color {
        let (r, g, b) = self.to_linear_rgb();
        Color::rgb(
            linear_to_srgb(r).clamp(0.0, 1.0),
            linear_to_srgb(g).clamp(0.0, 1.0),
            linear_to_srgb(b).clamp(0.0, 1.0),
        )
    }

    pub fn to_oklch(self) -> Oklch {
        Oklch {
            l: self.l,
            c: (self.a * self.a + self.b * self.b).sqrt(),
            h: self.b.atan2(self.a).to_degrees().rem_euclid(360.0),
        }
    }
}

/// Тот же OKLab, но в полярных координатах: светлота, насыщенность, оттенок.
///
/// В этом виде удобно строить палитру: фиксируем оттенок бренда и меняем
/// только светлоту, получая согласованный набор поверхностей.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    /// Оттенок в градусах, 0..360.
    pub h: f64,
}

impl Oklch {
    pub const fn new(l: f64, c: f64, h: f64) -> Self {
        Self { l, c, h }
    }

    pub fn to_oklab(self) -> Oklab {
        let h = self.h.to_radians();
        Oklab {
            l: self.l,
            a: self.c * h.cos(),
            b: self.c * h.sin(),
        }
    }

    /// Ближайший изображаемый цвет с тем же оттенком и светлотой.
    ///
    /// Не всякая пара «светлота и насыщенность» существует в sRGB: например,
    /// ярко-зелёного с высокой насыщенностью при средней светлоте попросту
    /// нет. Наивное решение — обрезать каналы — меняет оттенок: зелёный
    /// уезжает в салатовый, и палитра перестаёт соответствовать выбранному
    /// цвету.
    ///
    /// Поэтому насыщенность снижается ровно настолько, чтобы цвет уместился в
    /// охват, а оттенок и светлота сохраняются. Так же поступает CSS Color 4.
    pub fn to_color(self) -> Color {
        if self.to_oklab().is_in_gamut() {
            return self.to_oklab().to_color();
        }

        // Двоичный поиск по насыщенности: при нулевой насыщенности цвет
        // изображается всегда, значит решение существует.
        let (mut lo, mut hi) = (0.0, self.c);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if Oklch::new(self.l, mid, self.h).to_oklab().is_in_gamut() {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        Oklch::new(self.l, lo, self.h).to_oklab().to_color()
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_hex_forms() {
        assert_eq!(Color::from_hex("#fff").unwrap(), Color::WHITE);
        assert_eq!(Color::from_hex("#ffffff").unwrap(), Color::WHITE);
        assert_eq!(Color::from_hex("#000000").unwrap(), Color::BLACK);

        let half = Color::from_hex("#ffffff80").unwrap();
        assert!((half.a - 0.5019).abs() < 0.001);

        let short_alpha = Color::from_hex("#f00f").unwrap();
        assert_eq!(short_alpha, Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(
            Color::from_hex("ffffff"),
            Err(ColorParseError::MissingHash("ffffff".into()))
        );
        assert_eq!(Color::from_hex("#ff"), Err(ColorParseError::BadLength(2)));
        assert_eq!(
            Color::from_hex("#gggggg"),
            Err(ColorParseError::BadDigit('g'))
        );
    }

    #[test]
    fn hex_survives_a_round_trip() {
        for hex in ["#7c3aed", "#00ff88", "#123456", "#abcdef40"] {
            let c = Color::from_hex(hex).unwrap();
            assert_eq!(c.to_hex(), hex);
        }
    }

    #[test]
    fn oklab_survives_a_round_trip() {
        for hex in ["#7c3aed", "#00ff88", "#123456", "#ffffff", "#000000"] {
            let c = Color::from_hex(hex).unwrap();
            let back = c.to_oklab().to_color();
            assert!(
                (c.r - back.r).abs() < 1e-6
                    && (c.g - back.g).abs() < 1e-6
                    && (c.b - back.b).abs() < 1e-6,
                "{hex} превратился в {back}"
            );
        }
    }

    #[test]
    fn oklch_survives_a_round_trip() {
        let c = Color::from_hex("#7c3aed").unwrap();
        let back = c.to_oklch().to_color();
        assert!((c.r - back.r).abs() < 1e-6 && (c.b - back.b).abs() < 1e-6);
    }

    #[test]
    fn white_and_black_have_the_extreme_luminance() {
        assert!((Color::WHITE.relative_luminance() - 1.0).abs() < 1e-9);
        assert!(Color::BLACK.relative_luminance().abs() < 1e-9);
    }

    #[test]
    fn contrast_of_black_on_white_is_twenty_one() {
        let ratio = Color::BLACK.contrast_ratio(Color::WHITE);
        assert!((ratio - 21.0).abs() < 0.01, "получилось {ratio}");
    }

    #[test]
    fn contrast_is_symmetric_and_self_contrast_is_one() {
        let a = Color::from_hex("#7c3aed").unwrap();
        let b = Color::from_hex("#f0f0f0").unwrap();
        assert!((a.contrast_ratio(b) - b.contrast_ratio(a)).abs() < 1e-12);
        assert!((a.contrast_ratio(a) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn best_foreground_picks_the_readable_option() {
        assert_eq!(
            Color::from_hex("#111111").unwrap().best_foreground(),
            Color::WHITE
        );
        assert_eq!(
            Color::from_hex("#eeeeee").unwrap().best_foreground(),
            Color::BLACK
        );
    }

    #[test]
    fn lighten_and_darken_move_lightness_in_the_right_direction() {
        let base = Color::from_hex("#7c3aed").unwrap();
        assert!(base.lighten(0.2).to_oklch().l > base.to_oklch().l);
        assert!(base.darken(0.2).to_oklch().l < base.to_oklch().l);
    }

    #[test]
    fn lightness_is_clamped_instead_of_wrapping() {
        let base = Color::from_hex("#7c3aed").unwrap();
        let too_light = base.lighten(5.0);
        assert!(too_light.to_oklch().l <= 1.0 + 1e-9);
        let too_dark = base.darken(5.0);
        assert!(too_dark.to_oklch().l >= -1e-9);
    }

    #[test]
    fn mixing_hits_both_ends_and_interpolates_alpha() {
        let a = Color::from_hex("#ff0000").unwrap();
        let b = Color::from_hex("#0000ff").unwrap().with_alpha(0.0);

        assert_eq!(a.mix(b, 0.0).to_hex(), a.to_hex());
        assert!((a.mix(b, 0.5).a - 0.5).abs() < 1e-9);
        assert!((a.mix(b, 1.0).a - 0.0).abs() < 1e-9);
    }

    #[test]
    fn out_of_gamut_colours_keep_their_hue() {
        // Насыщенного зелёного при средней светлоте в sRGB не существует.
        // Цвет обязан потерять насыщенность, но не оттенок.
        let wanted = Oklch::new(0.5, 0.35, 145.0);
        let got = wanted.to_color().to_oklch();

        assert!(
            (got.h - wanted.h).abs() < 1.0,
            "оттенок уехал на {}°",
            (got.h - wanted.h).abs()
        );
        assert!((got.l - wanted.l).abs() < 0.01, "светлота уехала");
        assert!(got.c < wanted.c, "насыщенность должна была снизиться");
    }

    #[test]
    fn colours_inside_the_gamut_are_left_alone() {
        let inside = Oklch::new(0.6, 0.1, 250.0);
        let got = inside.to_color().to_oklch();
        assert!(
            (got.c - inside.c).abs() < 0.002,
            "насыщенность изменилась зря"
        );
    }

    #[test]
    fn gamut_mapping_survives_the_extremes() {
        // Ни чёрный, ни белый, ни абсурдная насыщенность не должны ронять поиск.
        for (l, c, h) in [(0.0, 0.4, 30.0), (1.0, 0.4, 200.0), (0.5, 10.0, 90.0)] {
            let color = Oklch::new(l, c, h).to_color();
            assert!((0.0..=1.0).contains(&color.r), "{l}/{c}/{h} -> {color}");
        }
    }

    #[test]
    fn with_chroma_zero_produces_a_grey() {
        let grey = Color::from_hex("#7c3aed").unwrap().with_chroma(0.0);
        assert!(
            (grey.r - grey.g).abs() < 0.01 && (grey.g - grey.b).abs() < 0.01,
            "{grey}"
        );
    }

    #[test]
    fn rgb8_matches_the_same_hex_colour() {
        assert_eq!(
            Color::from_rgb8(0x7c, 0x3a, 0xed),
            Color::from_hex("#7c3aed").unwrap()
        );
    }

    #[test]
    fn css_rgba_is_well_formed() {
        let c = Color::rgba(1.0, 0.0, 0.5, 0.25);
        assert_eq!(c.to_css_rgba(), "rgba(255, 0, 128, 0.250)");
    }
}
