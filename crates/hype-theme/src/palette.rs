//! Построение палитры из одного акцентного цвета.
//!
//! Пользователь выбирает один цвет — всё остальное считается. Нейтральные
//! поверхности получают чуть-чуть акцентного оттенка (приём, знакомый по
//! Material You и libadwaita): интерфейс перестаёт выглядеть «мёртвым серым»,
//! но и не рябит.
//!
//! Каждая пара «текст на фоне» проверяется на контраст по WCAG, поэтому даже
//! неудачно выбранный акцент остаётся читаемым.

use serde::{Deserialize, Serialize};

use crate::color::{Color, Oklch};

/// Светлая или тёмная схема.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Variant {
    #[default]
    Dark,
    Light,
}

impl Variant {
    pub fn is_dark(self) -> bool {
        matches!(self, Variant::Dark)
    }

    /// Противоположная схема — для переключателя «день/ночь».
    pub fn toggled(self) -> Self {
        match self {
            Variant::Dark => Variant::Light,
            Variant::Light => Variant::Dark,
        }
    }
}

/// Минимальный контраст для основного текста по WCAG AA.
pub const CONTRAST_TEXT: f64 = 4.5;
/// Минимальный контраст для крупного текста и элементов управления.
pub const CONTRAST_UI: f64 = 3.0;

/// Насыщенность нейтральных поверхностей: заметно ровно настолько, чтобы серый
/// перестал быть безжизненным.
const NEUTRAL_CHROMA: f64 = 0.014;

/// Светлоты слоёв интерфейса для одной схемы.
struct Levels {
    bg: f64,
    surface: f64,
    raised: f64,
    overlay: f64,
    fg: f64,
    fg_dim: f64,
    fg_disabled: f64,
    border: f64,
    /// Целевая светлота акцента, когда он работает текстом.
    accent_text: f64,
}

impl Levels {
    const DARK: Levels = Levels {
        bg: 0.180,
        surface: 0.235,
        raised: 0.275,
        overlay: 0.315,
        fg: 0.965,
        fg_dim: 0.760,
        fg_disabled: 0.520,
        border: 0.380,
        accent_text: 0.800,
    };

    const LIGHT: Levels = Levels {
        bg: 0.968,
        surface: 0.995,
        raised: 1.000,
        overlay: 1.000,
        fg: 0.240,
        fg_dim: 0.460,
        fg_disabled: 0.660,
        border: 0.870,
        accent_text: 0.500,
    };

    fn for_variant(variant: Variant) -> &'static Levels {
        match variant {
            Variant::Dark => &Levels::DARK,
            Variant::Light => &Levels::LIGHT,
        }
    }
}

/// Границы светлоты для акцента в роли заливки.
///
/// Сам акцент не переводится к единой светлоте: это превратило бы жёлтый в
/// оливковый, а голубой — в синий, то есть отняло бы у пользователя ровно тот
/// цвет, который он выбрал. Границы нужны лишь для крайностей: почти чёрная
/// или почти белая кнопка одинаково непригодна.
const FILL_L_MIN: f64 = 0.45;
const FILL_L_MAX: f64 = 0.82;

/// Семантические цвета: оттенок и светлота заливки в OKLCH.
///
/// Светлота у каждого своя, по той же причине: предупреждение обязано
/// оставаться жёлтым, а ошибка — красной.
const SEMANTIC_SUCCESS: (f64, f64) = (145.0, 0.58);
const SEMANTIC_WARNING: (f64, f64) = (85.0, 0.80);
const SEMANTIC_ERROR: (f64, f64) = (27.0, 0.55);

/// Готовый набор цветов интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    /// Фон окна — самый дальний слой.
    pub bg: Color,
    /// Фон карточек, боковых панелей, списков.
    pub surface: Color,
    /// Приподнятый слой: заголовки окон, панель.
    pub surface_raised: Color,
    /// Слой поверх всего: меню, подсказки, диалоги.
    pub overlay: Color,

    /// Основной текст.
    pub fg: Color,
    /// Второстепенный текст: подписи, метаданные.
    pub fg_dim: Color,
    /// Недоступные элементы.
    pub fg_disabled: Color,

    /// Разделители и рамки.
    pub border: Color,
    /// Цвет тени, уже с альфой.
    pub shadow: Color,

    /// Акцент для текста и иконок на обычном фоне.
    pub accent: Color,
    /// Акцент как фон — заливка кнопки, выделение в списке.
    pub accent_bg: Color,
    /// Текст поверх `accent_bg`.
    pub on_accent: Color,

    pub success: Color,
    pub success_bg: Color,
    /// Текст поверх `success_bg`.
    pub on_success: Color,

    pub warning: Color,
    pub warning_bg: Color,
    /// Текст поверх `warning_bg`.
    pub on_warning: Color,

    pub error: Color,
    pub error_bg: Color,
    /// Текст поверх `error_bg`.
    pub on_error: Color,
}

impl Palette {
    /// Строит палитру из акцентного цвета и схемы.
    pub fn from_accent(accent: Color, variant: Variant) -> Self {
        let levels = Levels::for_variant(variant);
        let source = accent.to_oklch();
        let hue = source.h;
        let accent_chroma = source.c;
        let source_lightness = source.l;

        let neutral = |l: f64| Oklch::new(l, NEUTRAL_CHROMA, hue).to_color();

        let bg = neutral(levels.bg);
        let surface = neutral(levels.surface);
        let surface_raised = neutral(levels.raised);
        let overlay = neutral(levels.overlay);
        let border = neutral(levels.border);

        // Текст считаем относительно самого светлого фона в тёмной теме и
        // самого тёмного в светлой: если он читается на худшем слое, он
        // читается везде.
        let worst_bg = if variant.is_dark() { overlay } else { bg };

        let fg = ensure_contrast(neutral(levels.fg), worst_bg, CONTRAST_TEXT);
        let fg_dim = ensure_contrast(neutral(levels.fg_dim), worst_bg, CONTRAST_UI);
        let fg_disabled = neutral(levels.fg_disabled);

        let accent_text = Oklch::new(levels.accent_text, accent_chroma, hue).to_color();
        let accent = ensure_contrast(accent_text, worst_bg, CONTRAST_UI);

        // Заливка — это выбранный пользователем цвет, а не приведённый к общей
        // светлоте: он должен остаться узнаваемым.
        let fill_l = source_lightness.clamp(FILL_L_MIN, FILL_L_MAX);
        let accent_bg = Oklch::new(fill_l, accent_chroma, hue).to_color();
        let on_accent = accent_bg.best_foreground();

        let semantic = |(hue, fill_l): (f64, f64)| -> (Color, Color, Color) {
            let text = Oklch::new(levels.accent_text, 0.13, hue).to_color();
            let fill = Oklch::new(fill_l, 0.15, hue).to_color();
            (
                ensure_contrast(text, worst_bg, CONTRAST_UI),
                fill,
                fill.best_foreground(),
            )
        };

        let (success, success_bg, on_success) = semantic(SEMANTIC_SUCCESS);
        let (warning, warning_bg, on_warning) = semantic(SEMANTIC_WARNING);
        let (error, error_bg, on_error) = semantic(SEMANTIC_ERROR);

        // Тень в светлой теме мягче: на белом фоне чёрная тень выглядит грязью.
        let shadow = Color::BLACK.with_alpha(if variant.is_dark() { 0.55 } else { 0.18 });

        Palette {
            bg,
            surface,
            surface_raised,
            overlay,
            fg,
            fg_dim,
            fg_disabled,
            border,
            shadow,
            accent,
            accent_bg,
            on_accent,
            success,
            success_bg,
            on_success,
            warning,
            warning_bg,
            on_warning,
            error,
            error_bg,
            on_error,
        }
    }

    /// Проверяет читаемость палитры. Возвращает список проблемных пар —
    /// пустой список означает, что всё в порядке.
    ///
    /// Используется в тестах и в разделе настроек «свой цвет»: если
    /// пользователь выкрутил палитру во что-то нечитаемое, ему об этом скажут.
    pub fn contrast_report(&self) -> Vec<ContrastIssue> {
        let checks: [(&str, Color, Color, f64); 7] = [
            ("текст на фоне окна", self.fg, self.bg, CONTRAST_TEXT),
            ("текст на карточке", self.fg, self.surface, CONTRAST_TEXT),
            ("текст в меню", self.fg, self.overlay, CONTRAST_TEXT),
            ("второстепенный текст", self.fg_dim, self.bg, CONTRAST_UI),
            ("акцент на фоне окна", self.accent, self.bg, CONTRAST_UI),
            (
                "текст на кнопке",
                self.on_accent,
                self.accent_bg,
                CONTRAST_TEXT,
            ),
            (
                "текст на ошибке",
                self.on_error,
                self.error_bg,
                CONTRAST_TEXT,
            ),
        ];

        checks
            .into_iter()
            .filter_map(|(what, fg, bg, required)| {
                let actual = fg.contrast_ratio(bg);
                (actual < required).then_some(ContrastIssue {
                    what: what.to_string(),
                    actual,
                    required,
                })
            })
            .collect()
    }
}

/// Пара цветов, не прошедшая проверку контраста.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastIssue {
    pub what: String,
    pub actual: f64,
    pub required: f64,
}

impl std::fmt::Display for ContrastIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: контраст {:.2}, нужно минимум {:.1}",
            self.what, self.actual, self.required
        )
    }
}

/// Двигает светлоту `fg` прочь от фона, пока контраст не достигнет `required`.
///
/// Направление выбирается по фону: на тёмном фоне текст светлеет, на светлом —
/// темнеет. Шаг мелкий, чтобы не уехать в чистый белый и не потерять оттенок;
/// если даже предел не даёт нужного контраста, возвращается лучший найденный
/// вариант — это честнее, чем молча отдать нечитаемый цвет.
pub fn ensure_contrast(fg: Color, bg: Color, required: f64) -> Color {
    if fg.contrast_ratio(bg) >= required {
        return fg;
    }

    let lighten = bg.relative_luminance() < 0.5;
    let mut best = fg;
    let mut best_ratio = fg.contrast_ratio(bg);
    let mut lch = fg.to_oklch();

    for _ in 0..100 {
        lch.l = if lighten { lch.l + 0.01 } else { lch.l - 0.01 };
        if !(0.0..=1.0).contains(&lch.l) {
            break;
        }
        let candidate = lch.to_color().with_alpha(fg.a);
        let ratio = candidate.contrast_ratio(bg);
        if ratio > best_ratio {
            best = candidate;
            best_ratio = ratio;
        }
        if ratio >= required {
            return candidate;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Набор акцентов, на которых палитра обязана держаться: от очень тёмного
    /// до кислотного и почти серого.
    const ACCENTS: [&str; 8] = [
        "#7c3aed", // фиолетовый — фирменный
        "#3584e4", // синий
        "#2ec27e", // зелёный
        "#f6d32d", // жёлтый, самый сложный для контраста
        "#e01b24", // красный
        "#111111", // почти чёрный
        "#f5f5f5", // почти белый
        "#808080", // серый без насыщенности
    ];

    #[test]
    fn generated_palettes_are_always_readable() {
        for hex in ACCENTS {
            for variant in [Variant::Dark, Variant::Light] {
                let accent = Color::from_hex(hex).unwrap();
                let palette = Palette::from_accent(accent, variant);
                let issues = palette.contrast_report();
                assert!(
                    issues.is_empty(),
                    "акцент {hex} / {variant:?}: {}",
                    issues
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
    }

    #[test]
    fn a_saturated_accent_gets_white_text_and_a_yellow_one_gets_black() {
        // Выбор делается по контрасту, но результат — часть облика среды,
        // поэтому закреплён тестом.
        for variant in [Variant::Dark, Variant::Light] {
            let purple = Palette::from_accent(Color::from_hex("#7c3aed").unwrap(), variant);
            assert_eq!(purple.on_accent, Color::WHITE, "{variant:?}");

            let yellow = Palette::from_accent(Color::from_hex("#f6d32d").unwrap(), variant);
            assert_eq!(yellow.on_accent, Color::BLACK, "{variant:?}");
        }
    }

    #[test]
    fn the_accent_keeps_its_own_lightness() {
        // Жёлтая кнопка обязана остаться жёлтой, а не стать оливковой.
        let yellow = Color::from_hex("#f6d32d").unwrap();
        let palette = Palette::from_accent(yellow, Variant::Dark);
        let fill = palette.accent_bg.to_oklch();
        assert!(fill.l > 0.75, "жёлтый потемнел до {}", fill.l);
        assert!((fill.h - yellow.to_oklch().h).abs() < 1.0);
    }

    #[test]
    fn an_extreme_accent_is_pulled_into_a_usable_range() {
        for hex in ["#050505", "#fdfdfd"] {
            let palette = Palette::from_accent(Color::from_hex(hex).unwrap(), Variant::Dark);
            let l = palette.accent_bg.to_oklch().l;
            // Допуск на округление при переходе через sRGB.
            assert!(
                ((FILL_L_MIN - 1e-6)..=(FILL_L_MAX + 1e-6)).contains(&l),
                "{hex} дал непригодную заливку со светлотой {l}"
            );
        }
    }

    #[test]
    fn warning_stays_yellow_and_error_stays_red() {
        let palette = Palette::from_accent(Color::from_hex("#7c3aed").unwrap(), Variant::Dark);
        assert!((palette.warning_bg.to_oklch().h - SEMANTIC_WARNING.0).abs() < 1.0);
        assert!(
            palette.warning_bg.to_oklch().l > 0.7,
            "предупреждение потускнело"
        );
        assert!((palette.error_bg.to_oklch().h - SEMANTIC_ERROR.0).abs() < 1.0);
        assert_eq!(palette.on_warning, Color::BLACK);
        assert_eq!(palette.on_error, Color::WHITE);
    }

    #[test]
    fn dark_palette_layers_get_lighter_as_they_rise() {
        let palette = Palette::from_accent(Color::from_hex("#7c3aed").unwrap(), Variant::Dark);
        let l = |c: Color| c.to_oklch().l;
        assert!(l(palette.bg) < l(palette.surface));
        assert!(l(palette.surface) < l(palette.surface_raised));
        assert!(l(palette.surface_raised) < l(palette.overlay));
    }

    #[test]
    fn light_palette_layers_get_lighter_as_they_rise_too() {
        let palette = Palette::from_accent(Color::from_hex("#7c3aed").unwrap(), Variant::Light);
        let l = |c: Color| c.to_oklch().l;
        assert!(l(palette.bg) <= l(palette.surface));
        assert!(l(palette.surface) <= l(palette.overlay));
    }

    #[test]
    fn dark_and_light_are_actually_different() {
        let accent = Color::from_hex("#7c3aed").unwrap();
        let dark = Palette::from_accent(accent, Variant::Dark);
        let light = Palette::from_accent(accent, Variant::Light);
        assert!(dark.bg.relative_luminance() < 0.1);
        assert!(light.bg.relative_luminance() > 0.8);
    }

    #[test]
    fn surfaces_keep_the_accent_hue() {
        let accent = Color::from_hex("#2ec27e").unwrap();
        let palette = Palette::from_accent(accent, Variant::Dark);
        let dh = (palette.surface.to_oklch().h - accent.to_oklch().h).abs();
        assert!(dh < 1.0, "поверхность потеряла оттенок акцента: {dh}°");
    }

    #[test]
    fn surfaces_are_nearly_neutral() {
        let accent = Color::from_hex("#e01b24").unwrap();
        let palette = Palette::from_accent(accent, Variant::Dark);
        assert!(
            palette.surface.to_oklch().c <= NEUTRAL_CHROMA + 1e-6,
            "поверхность слишком цветная"
        );
    }

    #[test]
    fn ensure_contrast_returns_the_input_when_it_is_already_fine() {
        let fg = Color::WHITE;
        let bg = Color::BLACK;
        assert_eq!(ensure_contrast(fg, bg, CONTRAST_TEXT), fg);
    }

    #[test]
    fn ensure_contrast_lightens_on_dark_and_darkens_on_light() {
        let mid = Color::from_hex("#767676").unwrap();

        let on_dark = ensure_contrast(mid, Color::from_hex("#202020").unwrap(), 7.0);
        assert!(on_dark.to_oklch().l > mid.to_oklch().l);

        let on_light = ensure_contrast(mid, Color::from_hex("#fafafa").unwrap(), 7.0);
        assert!(on_light.to_oklch().l < mid.to_oklch().l);
    }

    #[test]
    fn ensure_contrast_preserves_alpha() {
        let fg = Color::from_hex("#767676").unwrap().with_alpha(0.5);
        let out = ensure_contrast(fg, Color::BLACK, 12.0);
        assert!((out.a - 0.5).abs() < 1e-9);
    }

    #[test]
    fn impossible_contrast_returns_the_best_effort_not_a_panic() {
        // Контраст 21 достижим только для чёрного на белом; на среднем сером
        // такого нет ни у одного цвета.
        let out = ensure_contrast(
            Color::from_hex("#808080").unwrap(),
            Color::from_hex("#808080").unwrap(),
            21.0,
        );
        assert!(out.contrast_ratio(Color::from_hex("#808080").unwrap()) >= 1.0);
    }

    #[test]
    fn variant_toggles_both_ways() {
        assert_eq!(Variant::Dark.toggled(), Variant::Light);
        assert_eq!(Variant::Light.toggled().toggled(), Variant::Light);
    }
}
