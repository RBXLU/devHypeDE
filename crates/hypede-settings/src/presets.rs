//! Готовые акценты и подписи к настройкам.

use hype_theme::Color;

/// Название и цвет одного готового акцента.
pub struct Preset {
    pub name: &'static str,
    pub hex: &'static str,
}

/// Набор акцентов для быстрого выбора.
///
/// Это не ограничение: рядом с ними есть поле для любого цвета. Набор нужен,
/// чтобы у среды был узнаваемый облик из коробки и чтобы было с чего начать.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Фиолетовый",
        hex: "#7c3aed",
    },
    Preset {
        name: "Синий",
        hex: "#3584e4",
    },
    Preset {
        name: "Бирюзовый",
        hex: "#00b8a9",
    },
    Preset {
        name: "Зелёный",
        hex: "#2ec27e",
    },
    Preset {
        name: "Жёлтый",
        hex: "#f6d32d",
    },
    Preset {
        name: "Оранжевый",
        hex: "#ff7800",
    },
    Preset {
        name: "Красный",
        hex: "#e01b24",
    },
    Preset {
        name: "Розовый",
        hex: "#ff4d8d",
    },
];

/// Подпись к множителю скорости анимаций.
pub fn motion_label(scale: f64) -> String {
    if scale <= 0.0 {
        return "Выключены".to_string();
    }
    if (scale - 1.0).abs() < 0.01 {
        return "Обычная скорость".to_string();
    }
    if scale < 1.0 {
        format!("Быстрее в {:.1}×", 1.0 / scale)
    } else {
        format!("Медленнее в {scale:.1}×")
    }
}

/// Проверяет введённый вручную цвет.
///
/// Возвращает либо цвет, либо понятное человеку объяснение, что не так: поле
/// ввода цвета — единственное место настроек, где легко ошибиться.
pub fn parse_accent(input: &str) -> Result<Color, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Введите цвет, например #7c3aed".into());
    }

    // Частая ошибка: цвет скопирован без решётки.
    let normalised = if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    };

    Color::from_hex(&normalised).map_err(|err| match err {
        hype_theme::ColorParseError::BadLength(n) => {
            format!("Нужно 6 шестнадцатеричных цифр, а их {n}")
        }
        hype_theme::ColorParseError::BadDigit(c) => {
            format!("Символ «{c}» не является шестнадцатеричной цифрой")
        }
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_a_valid_colour() {
        for preset in PRESETS {
            assert!(
                Color::from_hex(preset.hex).is_ok(),
                "{} задан неверно",
                preset.name
            );
        }
    }

    #[test]
    fn every_preset_produces_a_readable_palette() {
        use hype_theme::{Palette, Variant};
        for preset in PRESETS {
            let color = Color::from_hex(preset.hex).unwrap();
            for variant in [Variant::Dark, Variant::Light] {
                let issues = Palette::from_accent(color, variant).contrast_report();
                assert!(issues.is_empty(), "{}: {issues:?}", preset.name);
            }
        }
    }

    #[test]
    fn motion_labels_describe_the_slider() {
        assert_eq!(motion_label(0.0), "Выключены");
        assert_eq!(motion_label(1.0), "Обычная скорость");
        assert_eq!(motion_label(2.0), "Медленнее в 2.0×");
        assert_eq!(motion_label(0.5), "Быстрее в 2.0×");
    }

    #[test]
    fn a_colour_without_a_hash_is_accepted() {
        assert_eq!(
            parse_accent("7c3aed").unwrap(),
            Color::from_hex("#7c3aed").unwrap()
        );
        assert_eq!(
            parse_accent("  #7c3aed  ").unwrap(),
            Color::from_hex("#7c3aed").unwrap()
        );
    }

    #[test]
    fn a_bad_colour_explains_itself_in_plain_words() {
        assert!(parse_accent("").unwrap_err().contains("например"));
        assert!(parse_accent("#12345").unwrap_err().contains("цифр"));
        assert!(parse_accent("#gg0000").unwrap_err().contains("«g»"));
    }
}
