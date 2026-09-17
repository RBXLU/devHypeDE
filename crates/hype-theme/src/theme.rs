//! Тема целиком: цвета, скругления, типографика, темп анимаций.
//!
//! Одна и та же тема описывает и композитор (рамки окон, тени, длительности),
//! и GTK-приложения (через сгенерированный CSS). Поэтому панель, файловый
//! менеджер и настройки не разъезжаются по стилю.

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::palette::{Palette, Variant};

/// Скругления углов в логических пикселях.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Radii {
    pub small: f64,
    pub medium: f64,
    pub large: f64,
    /// Скругление окон — его же использует композитор при отрисовке.
    pub window: f64,
}

impl Default for Radii {
    fn default() -> Self {
        // Крупные скругления — основа минималистичного облика: у формы почти
        // нет других деталей, поэтому именно скругление задаёт характер.
        Self {
            small: 8.0,
            medium: 14.0,
            large: 22.0,
            window: 16.0,
        }
    }
}

/// Шрифты интерфейса.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Typography {
    pub family: String,
    pub monospace_family: String,
    /// Базовый размер в пунктах.
    pub size_pt: f64,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            // Эти шрифты есть в CachyOS из коробки; если их нет, fontconfig
            // подставит ближайший.
            family: "Inter".into(),
            monospace_family: "JetBrains Mono".into(),
            size_pt: 10.5,
        }
    }
}

/// Настройки движения.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    /// Общий множитель длительности. 0 полностью выключает анимации —
    /// это же значение выставляет режим доступности «уменьшить движение».
    pub scale: f64,
    /// Базовая длительность перехода в миллисекундах.
    pub base_ms: u64,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            scale: 1.0,
            base_ms: 220,
        }
    }
}

impl Motion {
    /// Длительность с учётом множителя.
    pub fn duration(&self, ms: u64) -> std::time::Duration {
        let scaled = (ms as f64 * self.scale.max(0.0)).round() as u64;
        std::time::Duration::from_millis(scaled)
    }

    /// Выключено ли движение полностью.
    pub fn is_disabled(&self) -> bool {
        self.scale <= 0.0
    }
}

/// Прозрачность и размытие слоёв.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    /// Непрозрачность панели, 0..1.
    pub panel_opacity: f64,
    /// Непрозрачность всплывающих окон, 0..1.
    pub popover_opacity: f64,
    /// Радиус размытия под полупрозрачными слоями, в пикселях. 0 — без размытия.
    pub blur_radius: f64,
    /// Насколько окно без фокуса тускнеет, 0..1.
    pub inactive_dim: f64,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            panel_opacity: 0.92,
            popover_opacity: 0.98,
            blur_radius: 32.0,
            // Неактивное окно почти не тускнеет: в светлой теме заметное
            // затемнение выглядит поломкой, а не подсказкой.
            inactive_dim: 0.04,
        }
    }
}

/// Тема HypeDE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Схема окон приложений.
    pub variant: Variant,
    /// Схема оболочки. По умолчанию тёмная независимо от схемы окон.
    pub shell_variant: Variant,
    pub accent: Color,
    pub radii: Radii,
    pub typography: Typography,
    pub motion: Motion,
    pub effects: Effects,
    /// Базовый шаг сетки отступов в пикселях.
    pub spacing: f64,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// Основная тема среды: светлая, спокойная, с синим акцентом.
    ///
    /// Облик выбран минималистичным сознательно: чем меньше у интерфейса
    /// собственных деталей, тем меньше он спорит с содержимым окон.
    pub fn light() -> Self {
        Self {
            name: "HypeDE Light".into(),
            variant: Variant::Light,
            shell_variant: Variant::Dark,
            accent: Color::from_rgb8(0x1a, 0x73, 0xe8),
            radii: Radii::default(),
            typography: Typography::default(),
            motion: Motion::default(),
            effects: Effects::default(),
            spacing: 6.0,
        }
    }

    /// Тёмная версия основной темы.
    pub fn dark() -> Self {
        Self {
            name: "HypeDE Dark".into(),
            variant: Variant::Dark,
            ..Self::light()
        }
    }

    /// Прежняя фиолетовая тема — осталась как готовый вариант оформления.
    pub fn violet_night() -> Self {
        Self {
            name: "HypeDE Violet".into(),
            variant: Variant::Dark,
            accent: Color::from_rgb8(0x7c, 0x3a, 0xed),
            ..Self::light()
        }
    }

    /// Вычисляет палитру приложений. Специально не кэшируется в структуре:
    /// тема сериализуется в конфиг, и хранить в нём производные цвета — верный
    /// способ однажды получить палитру, не соответствующую акценту.
    pub fn palette(&self) -> Palette {
        Palette::from_accent(self.accent, self.variant)
    }

    /// Палитра оболочки — полки, поиска приложений и панели состояния.
    ///
    /// Оболочка живёт поверх обоев, а не поверх окна, поэтому её цвета не
    /// обязаны совпадать с цветами приложений. Тёмная подложка одинаково
    /// уверенно читается и на светлом снимке неба, и на тёмной фотографии, и
    /// не спорит со светлыми окнами поверх неё.
    pub fn shell_palette(&self) -> Palette {
        Palette::from_accent(self.accent, self.shell_variant)
    }

    /// Отступ в `steps` шагах базовой сетки.
    pub fn space(&self, steps: f64) -> f64 {
        self.spacing * steps
    }

    /// CSS для GTK4. Переопределяет именованные цвета libadwaita, поэтому
    /// стандартные виджеты подхватывают тему без единой правки в приложениях.
    pub fn to_gtk_css(&self) -> String {
        let p = self.palette();
        let shell = self.shell_palette();
        let mut css = String::new();

        css.push_str(&format!(
            "/* Сгенерировано HypeDE — тема «{}». Правки будут перезаписаны. */\n\n",
            self.name
        ));

        let mut define = |name: &str, color: Color| {
            css.push_str(&format!("@define-color {name} {};\n", color.to_css_rgba()));
        };

        // Именованные цвета libadwaita.
        define("accent_color", p.accent);
        define("accent_bg_color", p.accent_bg);
        define("accent_fg_color", p.on_accent);
        define("destructive_color", p.error);
        define("destructive_bg_color", p.error_bg);
        define("destructive_fg_color", p.on_error);
        define("success_color", p.success);
        define("success_bg_color", p.success_bg);
        define("success_fg_color", p.on_success);
        define("warning_color", p.warning);
        define("warning_bg_color", p.warning_bg);
        define("warning_fg_color", p.on_warning);
        define("error_color", p.error);
        define("error_bg_color", p.error_bg);
        define("error_fg_color", p.on_error);

        define("window_bg_color", p.bg);
        define("window_fg_color", p.fg);
        define("view_bg_color", p.surface);
        define("view_fg_color", p.fg);
        define("headerbar_bg_color", p.surface_raised);
        define("headerbar_fg_color", p.fg);
        define("headerbar_border_color", p.border);
        define("headerbar_backdrop_color", p.bg);
        define("sidebar_bg_color", p.surface);
        define("sidebar_fg_color", p.fg);
        define("sidebar_border_color", p.border);
        define("card_bg_color", p.surface_raised);
        define("card_fg_color", p.fg);
        define("dialog_bg_color", p.overlay);
        define("dialog_fg_color", p.fg);
        define("popover_bg_color", p.overlay);
        define("popover_fg_color", p.fg);
        define("shade_color", p.shadow);
        define("scrollbar_outline_color", p.border);

        // Собственные цвета HypeDE — их используют панель и файловый менеджер.
        define("hype_bg", p.bg);
        define("hype_surface", p.surface);
        define("hype_surface_raised", p.surface_raised);
        define("hype_overlay", p.overlay);
        define("hype_fg", p.fg);
        define("hype_fg_dim", p.fg_dim);
        define("hype_fg_disabled", p.fg_disabled);
        define("hype_border", p.border);
        define("hype_accent", p.accent);
        define("hype_accent_bg", p.accent_bg);
        define(
            "hype_panel_bg",
            p.surface_raised.with_alpha(self.effects.panel_opacity),
        );
        define(
            "hype_popover_bg",
            p.overlay.with_alpha(self.effects.popover_opacity),
        );

        css.push_str(&format!(
            "\n* {{\n  font-family: \"{}\", sans-serif;\n  font-size: {}pt;\n}}\n",
            self.typography.family, self.typography.size_pt
        ));
        css.push_str(&format!(
            "\nmonospace, .monospace {{\n  font-family: \"{}\", monospace;\n}}\n",
            self.typography.monospace_family
        ));

        css.push_str(&format!(
            r#"
/* Базовые виджеты.
   Именованных цветов выше достаточно приложениям на libadwaita, но обычное
   GTK-приложение о них не знает — ему нужны настоящие правила. */
window,
.background,
dialog {{
  background-color: @hype_bg;
  color: @hype_fg;
}}

/* Шапке окна нужны сразу три селектора: GTK рисует её фон на вложенном
   windowhandle, а часть приложений вешает класс .titlebar на свой виджет. */
headerbar,
headerbar > windowhandle,
.titlebar {{
  background-image: none;
  background-color: @hype_surface_raised;
  color: @hype_fg;
  border-bottom: 1px solid @hype_border;
}}

/* Скругление окна рисует сам клиент: узел decoration отвечает за рамку и
   тень вокруг содержимого. */
decoration {{
  border-radius: {window_radius}px;
  box-shadow: 0 6px 28px {shadow};
}}

decoration:backdrop {{
  box-shadow: 0 2px 12px {shadow};
}}

headerbar,
headerbar > windowhandle {{
  border-top-left-radius: {window_radius}px;
  border-top-right-radius: {window_radius}px;
}}

headerbar:backdrop,
headerbar > windowhandle:backdrop,
.titlebar:backdrop {{
  background-color: @hype_bg;
  color: @hype_fg_dim;
}}

windowcontrols button {{
  background: none;
  background-image: none;
  border-color: transparent;
  color: @hype_fg;
}}

windowcontrols button:hover {{
  background-color: {hover_raised};
}}

list,
listview,
columnview,
.view,
textview text {{
  background-color: @hype_surface;
  color: @hype_fg;
}}

list > row:selected,
listview > row:selected,
row:selected {{
  background-color: {selection};
  color: @hype_fg;
}}

list > row:hover,
listview > row:hover {{
  background-color: {hover};
}}

.navigation-sidebar {{
  background-color: @hype_surface;
  color: @hype_fg;
}}

popover > contents,
menu,
.menu {{
  background-color: @hype_popover_bg;
  color: @hype_fg;
  border: 1px solid @hype_border;
  border-radius: {medium}px;
}}

/* background-image: none обязателен: Adwaita заливает кнопки градиентом,
   который иначе перекрывает наш цвет и оставляет светлый прямоугольник. */
button {{
  background-image: none;
  background-color: @hype_surface_raised;
  color: @hype_fg;
  border: 1px solid @hype_border;
  border-radius: {small}px;
}}

button:hover {{
  background-color: {hover_raised};
}}

button.flat {{
  background: none;
  background-image: none;
  border-color: transparent;
}}

button:disabled {{
  color: @hype_fg_disabled;
}}

entry,
spinbutton {{
  background-image: none;
  background-color: @hype_surface;
  color: @hype_fg;
  border: 1px solid @hype_border;
  border-radius: {small}px;
}}

entry:focus-within {{
  border-color: @hype_accent;
}}

separator {{
  background-color: @hype_border;
}}

scrollbar {{
  background-color: transparent;
}}

scrollbar slider {{
  background-color: @hype_fg_disabled;
  border-radius: {small}px;
}}

tooltip {{
  background-color: @hype_overlay;
  color: @hype_fg;
  border-radius: {small}px;
}}

/* Элементы, которые GTK красит собственным акцентом. Без этих правил в
   середине нашей темы остаются синие ползунки и переключатели. */
scale trough {{
  background-color: @hype_border;
}}

scale highlight,
progressbar progress,
levelbar block.filled {{
  background-image: none;
  background-color: @hype_accent_bg;
}}

scale slider {{
  background-image: none;
  background-color: @hype_fg;
  border-color: @hype_border;
}}

switch {{
  background-image: none;
  background-color: @hype_border;
}}

switch:checked {{
  background-image: none;
  background-color: @hype_accent_bg;
}}

check:checked,
radio:checked,
checkbutton check:checked {{
  background-image: none;
  background-color: @hype_accent_bg;
  color: {on_accent};
}}

stackswitcher button:checked,
.navigation-sidebar row:selected {{
  background-color: {selection};
}}

/* ---- Оболочка ----
   Полка, поиск приложений и панель состояния живут поверх обоев, поэтому у
   них своя, всегда тёмная палитра: она одинаково читается и на светлом небе,
   и на тёмной фотографии. */

.hype-shell {{
  color: {shell_fg};
}}

/* Полка прижата к краю экрана во всю ширину: это край рабочего стола, а не
   плавающая панель, поэтому скруглений у неё нет. */
.hype-shelf {{
  background-color: {shelf_bg};
  color: {shell_fg};
}}

/* Кнопка запуска — круг у левого края. */
.hype-launcher-button {{
  background-image: none;
  background-color: {shell_raised};
  color: {shell_fg};
  border: none;
  border-radius: 999px;
  min-width: {round_button}px;
  min-height: {round_button}px;
  padding: 0;
}}

.hype-launcher-button:hover {{
  background-color: {shell_hover_strong};
}}

/* Значки приложений на полке: круглая подложка, как у значков в лаунчере. */
.hype-shelf-button {{
  background: none;
  background-image: none;
  border: none;
  border-radius: 999px;
  padding: {shelf_button_padding}px;
}}

.hype-shelf-button:hover {{
  background-color: {shell_hover};
}}

/* Запущенное приложение отмечается точкой под значком — так же, как это
   привычно на полке любой системы с доком. */
.hype-running-dot {{
  background-color: {shell_fg};
  border-radius: 999px;
  min-width: 4px;
  min-height: 4px;
}}

.hype-running-dot.hype-hidden {{
  background-color: transparent;
}}

/* Область состояния справа — одна пилюля. */
.hype-tray {{
  background: none;
  background-image: none;
  border: none;
  color: {shell_fg};
  border-radius: 999px;
  padding: {tray_padding_v}px {tray_padding_h}px;
}}

.hype-tray:hover {{
  background-color: {shell_hover};
}}

/* Поиск приложений и панель быстрых настроек. */
.hype-launcher,
.hype-bubble {{
  background-color: {shell_bg};
  color: {shell_fg};
  border: 1px solid {shell_border};
  border-radius: {shell_radius}px;
}}

/* Фон всплывающей панели GTK рисует не на самом виджете, а на вложенном
   узле contents: без этого правила панель осталась бы светлой. */
popover.hype-bubble > contents {{
  background-color: {shell_bg};
  background-image: none;
  color: {shell_fg};
  border: 1px solid {shell_border};
  border-radius: {shell_radius}px;
  padding: 0;
}}

popover.hype-bubble > arrow {{
  background-color: {shell_bg};
  border-color: {shell_border};
}}

popover.hype-bubble label {{
  color: {shell_fg};
}}

entry.hype-search,
.hype-search entry,
.hype-search {{
  background-color: {shell_raised};
  background-image: none;
  color: {shell_fg};
  border: none;
  border-radius: 999px;
  padding: {search_padding}px {search_padding_h}px;
}}

entry.hype-search:focus-within {{
  background-color: {shell_hover_strong};
}}

/* Плитка приложения в сетке. */
.hype-app-tile {{
  background: none;
  background-image: none;
  border: none;
  border-radius: {medium}px;
  padding: {tile_padding}px;
  color: {shell_fg};
}}

.hype-app-tile:hover,
.hype-app-tile:selected {{
  background-color: {shell_hover};
}}

/* Круглая подложка под значком приложения: в оригинале значки тоже круглые,
   и именно это делает сетку узнаваемой. */
.hype-icon-circle {{
  background-color: {icon_circle};
  border-radius: 999px;
  padding: {icon_padding}px;
}}

/* Точка под значком запущенного приложения. */
.hype-running-dot {{
  margin-top: 1px;
}}

/* Круглые кнопки в панели быстрых настроек. */
.hype-round-button {{
  background-image: none;
  background-color: {shell_raised};
  color: {shell_fg};
  border: none;
  border-radius: 999px;
  min-width: {round_button}px;
  min-height: {round_button}px;
  padding: 0;
}}

.hype-round-button:hover {{
  background-color: {shell_hover_strong};
}}

.hype-round-button.hype-active {{
  background-color: {shell_accent};
  color: {shell_on_accent};
}}

/* Пилюля с надписью — «Выйти» и подобные. */
.hype-pill-button {{
  background-image: none;
  background-color: {shell_raised};
  color: {shell_fg};
  border: none;
  border-radius: 999px;
  padding: {shell_pill_v}px {shell_pill_h}px;
}}

.hype-pill-button:hover {{
  background-color: {shell_hover_strong};
}}

.hype-shell-dim {{
  color: {shell_fg_dim};
}}

/* Текущий рабочий стол на полке. */
.hype-workspace-active {{
  background-color: {shell_hover_strong};
  color: {shell_fg};
}}

.hype-shell scale trough {{
  background-color: {shell_raised};
}}

.hype-shell scale highlight {{
  background-color: {shell_accent};
}}

/* Общие правила HypeDE */

.hype-panel {{
  background-color: @hype_panel_bg;
  color: @hype_fg;
  border-radius: {panel_radius}px;
  padding: {panel_padding}px;
}}

.hype-card {{
  background-color: @hype_surface_raised;
  border: 1px solid @hype_border;
  border-radius: {medium}px;
  padding: {card_padding}px;
}}

.hype-pill {{
  border-radius: {large}px;
  padding: {pill_v}px {pill_h}px;
}}

.hype-dim {{
  color: @hype_fg_dim;
}}

.hype-accent {{
  color: @hype_accent;
}}

button.hype-primary {{
  background-color: @hype_accent_bg;
  color: {on_accent};
  border-radius: {small}px;
}}

.hype-selected {{
  background-color: {selection};
  border-radius: {small}px;
}}
"#,
            shell_fg = shell.fg.to_css_rgba(),
            shell_fg_dim = shell.fg_dim.to_css_rgba(),
            shell_bg = shell.surface.with_alpha(0.97).to_css_rgba(),
            shelf_bg = shell
                .bg
                .with_alpha(self.effects.panel_opacity)
                .to_css_rgba(),
            shell_raised = shell.fg.with_alpha(0.10).to_css_rgba(),
            shell_hover = shell.fg.with_alpha(0.08).to_css_rgba(),
            shell_hover_strong = shell.fg.with_alpha(0.16).to_css_rgba(),
            shell_border = shell.fg.with_alpha(0.08).to_css_rgba(),
            shell_accent = shell.accent.to_css_rgba(),
            shell_on_accent = shell.bg.to_css_rgba(),
            shell_radius = self.radii.large,
            icon_circle = Color::WHITE.to_css_rgba(),
            icon_padding = self.space(1.0),
            round_button = self.space(6.0),
            shell_pill_v = self.space(1.0),
            shell_pill_h = self.space(2.5),
            window_radius = self.radii.window,
            shadow = p.shadow.to_css_rgba(),
            shelf_button_padding = self.space(0.5),
            tray_padding_v = self.space(1.0),
            tray_padding_h = self.space(2.0),
            search_padding = self.space(1.5),
            search_padding_h = self.space(2.5),
            tile_padding = self.space(2.0),
            panel_radius = self.radii.large,
            panel_padding = self.space(1.0),
            medium = self.radii.medium,
            card_padding = self.space(2.0),
            large = self.radii.large,
            pill_v = self.space(0.5),
            pill_h = self.space(2.0),
            small = self.radii.small,
            on_accent = p.on_accent.to_css_rgba(),
            selection = p.accent_bg.with_alpha(0.28).to_css_rgba(),
            hover = p.fg.with_alpha(0.07).to_css_rgba(),
            hover_raised = p.fg.with_alpha(0.10).to_css_rgba(),
        ));

        css
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_theme_is_the_light_one() {
        let theme = Theme::default();
        assert_eq!(theme.variant, Variant::Light);
        assert!(theme.palette().contrast_report().is_empty());
    }

    #[test]
    fn every_bundled_theme_is_readable() {
        for theme in [Theme::light(), Theme::dark(), Theme::violet_night()] {
            let issues = theme.palette().contrast_report();
            assert!(issues.is_empty(), "{}: {issues:?}", theme.name);
        }
    }

    #[test]
    fn dark_and_light_differ_only_in_the_scheme() {
        let light = Theme::light();
        let dark = Theme::dark();
        assert_eq!(light.accent, dark.accent);
        assert_eq!(light.radii, dark.radii);
        assert_ne!(light.variant, dark.variant);
    }

    #[test]
    fn palette_follows_the_accent() {
        let theme = Theme {
            accent: Color::from_hex("#2ec27e").unwrap(),
            ..Theme::default()
        };
        let hue = theme.palette().accent.to_oklch().h;
        assert!((hue - theme.accent.to_oklch().h).abs() < 1.0);
    }

    #[test]
    fn theme_survives_a_toml_round_trip() {
        let theme = Theme::default();
        let text = toml::to_string_pretty(&theme).unwrap();
        let back: Theme = toml::from_str(&text).unwrap();
        assert_eq!(theme, back);
        // Цвет в конфиге должен быть узнаваемой строкой, а не таблицей.
        assert!(text.contains("accent = \"#1a73e8\""), "{text}");
    }

    #[test]
    fn partial_config_falls_back_to_defaults() {
        let theme: Theme = toml::from_str("name = \"Мой стиль\"\naccent = \"#ff0066\"").unwrap();
        assert_eq!(theme.name, "Мой стиль");
        assert_eq!(theme.accent, Color::from_hex("#ff0066").unwrap());
        assert_eq!(theme.radii, Radii::default());
        assert_eq!(theme.motion, Motion::default());
    }

    #[test]
    fn motion_scale_stretches_and_disables_animations() {
        let mut motion = Motion::default();
        assert_eq!(motion.duration(200).as_millis(), 200);

        motion.scale = 0.5;
        assert_eq!(motion.duration(200).as_millis(), 100);

        motion.scale = 0.0;
        assert!(motion.is_disabled());
        assert_eq!(motion.duration(200).as_millis(), 0);
    }

    #[test]
    fn negative_motion_scale_is_treated_as_disabled() {
        let motion = Motion {
            scale: -1.0,
            base_ms: 200,
        };
        assert_eq!(motion.duration(200).as_millis(), 0);
    }

    #[test]
    fn css_defines_the_colours_gtk_apps_expect() {
        let css = Theme::default().to_gtk_css();
        for name in [
            "accent_bg_color",
            "window_bg_color",
            "headerbar_bg_color",
            "popover_bg_color",
            "card_bg_color",
            "hype_panel_bg",
        ] {
            assert!(
                css.contains(&format!("@define-color {name} ")),
                "нет {name}"
            );
        }
    }

    #[test]
    fn css_carries_the_configured_font() {
        let mut theme = Theme::default();
        theme.typography.family = "Cantarell".into();
        theme.typography.size_pt = 12.5;
        let css = theme.to_gtk_css();
        assert!(css.contains("\"Cantarell\""));
        assert!(css.contains("12.5pt"));
    }

    #[test]
    fn css_styles_plain_gtk_widgets_too() {
        // Без этих правил приложение, не использующее libadwaita, осталось бы
        // в системной теме, и среда выглядела бы разнородной.
        let css = Theme::default().to_gtk_css();
        for selector in [
            "window,",
            "headerbar,",
            "headerbar > windowhandle,",
            "windowcontrols button {",
            "decoration {",
            "scale highlight,",
            "popover.hype-bubble > contents {",
            "switch:checked {",
            "button {",
            "entry,",
            "popover > contents,",
        ] {
            assert!(css.contains(selector), "нет правила для {selector}");
        }
    }

    #[test]
    fn interactive_widgets_drop_the_adwaita_gradient() {
        // Без background-image: none кнопка остаётся светлой поверх тёмной
        // темы — эту ошибку видно только глазами, поэтому она закреплена здесь.
        let css = Theme::default().to_gtk_css();
        let button_rule = css
            .split("button {")
            .nth(1)
            .expect("в CSS нет правила для кнопки");
        assert!(
            button_rule[..button_rule.find('}').unwrap()].contains("background-image: none"),
            "кнопка не сбрасывает градиент"
        );
    }

    #[test]
    fn css_styles_the_shelf_and_the_launcher() {
        let css = Theme::default().to_gtk_css();
        for selector in [
            ".hype-shelf {",
            ".hype-shelf-button {",
            ".hype-launcher-button {",
            ".hype-tray {",
            ".hype-launcher,",
            ".hype-icon-circle {",
            ".hype-round-button {",
        ] {
            assert!(css.contains(selector), "нет правила для {selector}");
        }
    }

    #[test]
    fn the_shell_stays_dark_in_a_light_theme() {
        // Полка лежит поверх обоев, а не поверх окна: её цвет не должен
        // меняться вместе со схемой приложений.
        let theme = Theme::light();
        assert_eq!(theme.variant, Variant::Light);
        assert_eq!(theme.shell_variant, Variant::Dark);
        assert!(theme.shell_palette().bg.relative_luminance() < 0.1);
        assert!(theme.palette().bg.relative_luminance() > 0.8);
    }

    #[test]
    fn the_shell_palette_is_readable_too() {
        for theme in [Theme::light(), Theme::dark(), Theme::violet_night()] {
            let issues = theme.shell_palette().contrast_report();
            assert!(issues.is_empty(), "{}: {issues:?}", theme.name);
        }
    }

    #[test]
    fn css_has_balanced_braces() {
        // Дешёвая защита от опечатки в шаблоне: несбалансированные скобки
        // ломают весь CSS, а GTK сообщает об этом крайне невнятно.
        let css = Theme::default().to_gtk_css();
        let open = css.matches('{').count();
        let close = css.matches('}').count();
        assert_eq!(open, close, "скобки в CSS не сходятся");
    }

    #[test]
    fn spacing_scales_by_steps() {
        let theme = Theme::default();
        assert_eq!(theme.space(2.0), theme.spacing * 2.0);
    }
}
