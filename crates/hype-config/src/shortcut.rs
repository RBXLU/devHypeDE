//! Горячие клавиши: разбор строк вида `Super+Shift+Q`.
//!
//! В конфиге сочетание — обычная строка, потому что править её должен человек,
//! а не редактор структур. Разбор намеренно снисходителен к регистру и
//! синонимам (`Mod4` = `Super` = `Win`), но не к опечаткам в названии клавиши:
//! молча проигнорированная привязка — худший вид ошибки в конфиге.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Ошибка разбора сочетания клавиш.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ShortcutParseError {
    #[error("пустое сочетание клавиш")]
    Empty,
    #[error("неизвестный модификатор: {0:?}")]
    UnknownModifier(String),
    #[error("неизвестная клавиша: {0:?}")]
    UnknownKey(String),
    #[error("в сочетании {0:?} нет самой клавиши, только модификаторы")]
    ModifiersOnly(String),
}

/// Набор зажатых модификаторов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Mods {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        logo: false,
        ctrl: false,
        alt: false,
        shift: false,
    };

    pub const LOGO: Mods = Mods {
        logo: true,
        ..Mods::NONE
    };

    pub fn with_logo(mut self) -> Self {
        self.logo = true;
        self
    }

    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn is_empty(self) -> bool {
        self == Mods::NONE
    }

    fn parse_into(&mut self, token: &str) -> Result<(), ShortcutParseError> {
        match token.to_ascii_lowercase().as_str() {
            "super" | "mod4" | "win" | "logo" | "meta" => self.logo = true,
            "ctrl" | "control" | "mod5" => self.ctrl = true,
            "alt" | "mod1" => self.alt = true,
            "shift" => self.shift = true,
            other => return Err(ShortcutParseError::UnknownModifier(other.to_string())),
        }
        Ok(())
    }
}

impl fmt::Display for Mods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Порядок фиксирован, чтобы одно и то же сочетание всегда выглядело
        // одинаково — и в конфиге, и в подсказках интерфейса.
        for (flag, name) in [
            (self.logo, "Super"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ] {
            if flag {
                write!(f, "{name}+")?;
            }
        }
        Ok(())
    }
}

/// Сама клавиша.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// Печатный символ: буква, цифра, знак.
    Char(char),
    /// Именованная клавиша: `Return`, `F5`, `Left`, `XF86AudioRaiseVolume`.
    Named(String),
}

impl Key {
    /// Разбирает имя клавиши в том виде, в каком его отдаёт xkb.
    ///
    /// Нужна композитору: он получает от xkb строку вроде `"q"`, `"Return"`
    /// или `"XF86AudioMute"` и должен сопоставить её с привязками из конфига.
    pub fn from_name(name: &str) -> Result<Self, ShortcutParseError> {
        parse_key(name)
    }

    /// Имя клавиши в терминах xkb — то, что понимает композитор.
    pub fn xkb_name(&self) -> String {
        match self {
            Key::Char(' ') => "space".to_string(),
            Key::Char(c) => c.to_string(),
            Key::Named(name) => name.clone(),
        }
    }
}

/// Именованные клавиши, которые разбор принимает. Список нарочно закрытый:
/// так опечатка `Retrun` становится ошибкой конфига, а не тихо мёртвой
/// привязкой.
const NAMED_KEYS: &[&str] = &[
    "Return",
    "Enter",
    "Space",
    "Tab",
    "Escape",
    "BackSpace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Left",
    "Right",
    "Up",
    "Down",
    "Print",
    "Menu",
    "Pause",
    "CapsLock",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
];

/// Сочетание клавиш целиком.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: Key,
}

impl Shortcut {
    pub fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }
}

impl FromStr for Shortcut {
    type Err = ShortcutParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(ShortcutParseError::Empty);
        }

        let tokens: Vec<&str> = trimmed
            .split('+')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();

        let Some((key_token, mod_tokens)) = tokens.split_last() else {
            return Err(ShortcutParseError::Empty);
        };

        let mut mods = Mods::NONE;
        for token in mod_tokens {
            mods.parse_into(token)?;
        }

        // Если последним токеном оказался модификатор, привязка бессмысленна.
        let mut probe = Mods::NONE;
        if probe.parse_into(key_token).is_ok() {
            return Err(ShortcutParseError::ModifiersOnly(trimmed.to_string()));
        }

        let key = parse_key(key_token)?;
        Ok(Shortcut { mods, key })
    }
}

fn parse_key(token: &str) -> Result<Key, ShortcutParseError> {
    // Мультимедийные клавиши приходят из xkb уже с префиксом XF86.
    if token.starts_with("XF86") {
        return Ok(Key::Named(token.to_string()));
    }

    if let Some(name) = NAMED_KEYS.iter().find(|n| n.eq_ignore_ascii_case(token)) {
        return Ok(Key::Named((*name).to_string()));
    }

    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_whitespace() => Ok(Key::Char(c.to_ascii_lowercase())),
        _ => Err(ShortcutParseError::UnknownKey(token.to_string())),
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.mods)?;
        match &self.key {
            Key::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            Key::Named(name) => write!(f, "{name}"),
        }
    }
}

impl Serialize for Shortcut {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Shortcut {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_combination() {
        let s: Shortcut = "Super+Q".parse().unwrap();
        assert_eq!(s.mods, Mods::LOGO);
        assert_eq!(s.key, Key::Char('q'));
    }

    #[test]
    fn parses_every_modifier_and_its_synonyms() {
        let a: Shortcut = "Super+Ctrl+Alt+Shift+K".parse().unwrap();
        assert_eq!(
            a.mods,
            Mods::NONE.with_logo().with_ctrl().with_alt().with_shift()
        );

        let b: Shortcut = "mod4+control+mod1+shift+k".parse().unwrap();
        assert_eq!(a, b, "синонимы модификаторов должны давать то же сочетание");
    }

    #[test]
    fn modifier_order_does_not_matter() {
        let a: Shortcut = "Shift+Super+D".parse().unwrap();
        let b: Shortcut = "Super+Shift+D".parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn is_case_insensitive_and_tolerates_spaces() {
        let a: Shortcut = "  super + shift + q  ".parse().unwrap();
        assert_eq!(a, "Super+Shift+Q".parse().unwrap());
    }

    #[test]
    fn parses_named_and_media_keys() {
        assert_eq!(
            "Super+Return".parse::<Shortcut>().unwrap().key,
            Key::Named("Return".into())
        );
        assert_eq!(
            "F5".parse::<Shortcut>().unwrap().key,
            Key::Named("F5".into())
        );
        assert_eq!(
            "XF86AudioRaiseVolume".parse::<Shortcut>().unwrap().key,
            Key::Named("XF86AudioRaiseVolume".into())
        );
    }

    #[test]
    fn a_shortcut_without_modifiers_is_allowed() {
        let s: Shortcut = "Print".parse().unwrap();
        assert!(s.mods.is_empty());
    }

    #[test]
    fn rejects_typos_instead_of_ignoring_them() {
        assert_eq!(
            "Super+Retrun".parse::<Shortcut>(),
            Err(ShortcutParseError::UnknownKey("Retrun".into()))
        );
        assert_eq!(
            "Hyper+Q".parse::<Shortcut>(),
            Err(ShortcutParseError::UnknownModifier("hyper".into()))
        );
    }

    #[test]
    fn rejects_empty_and_modifier_only_input() {
        assert_eq!("".parse::<Shortcut>(), Err(ShortcutParseError::Empty));
        assert_eq!("   ".parse::<Shortcut>(), Err(ShortcutParseError::Empty));
        assert_eq!(
            "Super+Shift".parse::<Shortcut>(),
            Err(ShortcutParseError::ModifiersOnly("Super+Shift".into()))
        );
    }

    #[test]
    fn display_is_canonical_and_reparses() {
        for input in ["shift+super+q", "ctrl+alt+Delete", "F11", "Super+space"] {
            let s: Shortcut = input.parse().unwrap();
            let shown = s.to_string();
            assert_eq!(shown.parse::<Shortcut>().unwrap(), s, "сломался {shown}");
        }
        assert_eq!(
            "shift+super+q".parse::<Shortcut>().unwrap().to_string(),
            "Super+Shift+Q"
        );
    }

    #[test]
    fn key_names_from_xkb_are_understood() {
        // Ровно те строки, которые приходят от xkb_keysym_get_name.
        assert_eq!(Key::from_name("q").unwrap(), Key::Char('q'));
        assert_eq!(Key::from_name("Q").unwrap(), Key::Char('q'));
        assert_eq!(Key::from_name("space").unwrap(), Key::Named("Space".into()));
        assert_eq!(
            Key::from_name("Return").unwrap(),
            Key::Named("Return".into())
        );
        assert_eq!(
            Key::from_name("XF86AudioMute").unwrap(),
            Key::Named("XF86AudioMute".into())
        );
        assert!(Key::from_name("NoSymbol").is_err());
    }

    #[test]
    fn xkb_name_maps_space_correctly() {
        let s: Shortcut = "Super+Space".parse().unwrap();
        assert_eq!(s.key.xkb_name(), "Space");
        assert_eq!(Key::Char(' ').xkb_name(), "space");
        assert_eq!(Key::Char('q').xkb_name(), "q");
    }

    #[test]
    fn serialises_as_a_plain_string() {
        let s: Shortcut = "Super+Shift+Q".parse().unwrap();
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"Super+Shift+Q\"");
        let back: Shortcut = serde_json::from_str("\"Super+Shift+Q\"").unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn deserialisation_reports_the_bad_value() {
        let err = serde_json::from_str::<Shortcut>("\"Super+Nope\"").unwrap_err();
        assert!(err.to_string().contains("Nope"), "{err}");
    }
}
