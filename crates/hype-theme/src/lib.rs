//! Тема и цветовая система HypeDE.
//!
//! Крейт решает три задачи:
//!
//! 1. честная работа с цветом (sRGB, OKLab, OKLCH, контраст по WCAG);
//! 2. построение всей палитры из одного акцентного цвета;
//! 3. выгрузка темы в CSS для GTK-приложений среды.
//!
//! # Пример
//!
//! ```
//! use hype_theme::{Theme, Color, Variant};
//!
//! let mut theme = Theme::default();
//! theme.accent = Color::from_hex("#2ec27e").unwrap();
//! theme.variant = Variant::Light;
//!
//! let palette = theme.palette();
//! // Палитра всегда читаема, какой бы акцент ни выбрал пользователь.
//! assert!(palette.contrast_report().is_empty());
//!
//! let css = theme.to_gtk_css();
//! assert!(css.contains("@define-color accent_bg_color"));
//! ```

#![deny(rust_2018_idioms)]

mod color;
mod palette;
mod theme;

pub use color::{Color, ColorParseError, Oklab, Oklch};
pub use palette::{ensure_contrast, ContrastIssue, Palette, Variant, CONTRAST_TEXT, CONTRAST_UI};
pub use theme::{Effects, Motion, Radii, Theme, Typography};
