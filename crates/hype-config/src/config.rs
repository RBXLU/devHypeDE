//! Файл настроек HypeDE.
//!
//! Формат — TOML: его правят руками, поэтому частичный конфиг обязан работать.
//! Любая пропущенная секция берётся из значений по умолчанию, и пользователь
//! может держать в файле только то, что действительно менял.

use std::time::Duration;

use hype_anim::{Curve, Easing, Spring};
use hype_theme::Theme;
use serde::{Deserialize, Serialize};

use crate::action::{Action, Direction, ScreenshotTarget};
use crate::shortcut::Shortcut;

/// Настройки клавиатуры, мыши и тачпада.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Раскладки через запятую, как в xkb: `"us,ru"`.
    pub keyboard_layout: String,
    /// Параметры xkb, например `"grp:alt_shift_toggle"` для переключения раскладки.
    pub keyboard_options: String,
    /// Задержка до автоповтора, мс.
    pub repeat_delay_ms: u32,
    /// Частота автоповтора, нажатий в секунду.
    pub repeat_rate: u32,
    /// Ускорение указателя, от -1 (медленно) до 1 (быстро).
    pub pointer_accel: f64,
    /// Естественная прокрутка (как на телефоне).
    pub natural_scroll: bool,
    /// Касание тачпада = клик.
    pub tap_to_click: bool,
    /// Фокус переходит под курсор без клика.
    pub focus_follows_mouse: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            keyboard_layout: "us,ru".into(),
            keyboard_options: "grp:alt_shift_toggle".into(),
            repeat_delay_ms: 400,
            repeat_rate: 30,
            pointer_accel: 0.0,
            natural_scroll: false,
            tap_to_click: true,
            focus_follows_mouse: false,
        }
    }
}

/// Раскладка окон.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutMode {
    /// Окна делят экран без перекрытий.
    #[default]
    Tiling,
    /// Окна свободно плавают, как в привычных средах.
    Floating,
}

/// Геометрия рабочего пространства.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub mode: LayoutMode,
    /// Зазор между окнами, px.
    pub gaps_inner: f64,
    /// Отступ от краёв экрана, px.
    pub gaps_outer: f64,
    /// Толщина рамки окна, px.
    pub border_width: f64,
    /// Количество рабочих столов.
    pub workspaces: u8,
    /// Доля экрана под главным окном в режиме плитки, 0.1..0.9.
    pub master_ratio: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            mode: LayoutMode::Tiling,
            gaps_inner: 8.0,
            gaps_outer: 12.0,
            border_width: 2.0,
            workspaces: 9,
            master_ratio: 0.55,
        }
    }
}

/// Одна анимация: включена ли она и по какому закону идёт.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimSpec {
    pub enabled: bool,
    pub curve: Curve,
}

impl AnimSpec {
    pub fn timed(ms: u64, easing: Easing) -> Self {
        Self {
            enabled: true,
            curve: Curve::timed(ms, easing),
        }
    }

    pub fn spring(spring: Spring) -> Self {
        Self {
            enabled: true,
            curve: Curve::Spring(spring),
        }
    }

    /// Кривая с учётом общего множителя темпа из темы.
    ///
    /// Множитель 0 (или выключенная анимация) сводит длительность к нулю —
    /// значение просто прыгает к цели.
    pub fn curve_scaled(&self, scale: f64) -> Curve {
        if !self.enabled || scale <= 0.0 {
            return Curve::Timed {
                duration: Duration::ZERO,
                easing: Easing::Linear,
            };
        }
        match self.curve {
            Curve::Timed { duration, easing } => Curve::Timed {
                duration: duration.mul_f64(scale),
                easing,
            },
            // Пружина замедляется уменьшением жёсткости: так сохраняется её
            // характер, чего не даёт простое растягивание времени.
            Curve::Spring(spring) => Curve::Spring(Spring {
                stiffness: spring.stiffness / (scale * scale).max(1e-6),
                ..spring
            }),
        }
    }
}

/// Набор анимаций среды.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
    /// Появление окна.
    pub window_open: AnimSpec,
    /// Закрытие окна.
    pub window_close: AnimSpec,
    /// Перемещение и изменение размера при перестроении раскладки.
    pub window_move: AnimSpec,
    /// Переключение рабочего стола.
    pub workspace_switch: AnimSpec,
    /// Появление панелей и меню.
    pub popup: AnimSpec,
    /// Вход и выход из обзора окон.
    pub overview: AnimSpec,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            // Окно распахивается с лёгким перелётом — это главный жест среды.
            window_open: AnimSpec::spring(Spring::BOUNCY),
            // Закрытие короткое и без колебаний: окна не должно быть жалко.
            window_close: AnimSpec::timed(140, Easing::EaseInQuad),
            // Перестроение раскладки — критическая пружина, без дрожания.
            window_move: AnimSpec::spring(Spring::SMOOTH),
            workspace_switch: AnimSpec::spring(Spring::SNAPPY),
            popup: AnimSpec::timed(160, Easing::EaseOutCubic),
            overview: AnimSpec::spring(Spring::SMOOTH),
        }
    }
}

/// Где висит панель.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PanelPosition {
    #[default]
    Top,
    Bottom,
}

/// Панель и её содержимое.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    pub enabled: bool,
    pub position: PanelPosition,
    pub height: u32,
    /// Модули слева направо. Неизвестные имена пропускаются с предупреждением.
    pub modules_left: Vec<String>,
    pub modules_center: Vec<String>,
    pub modules_right: Vec<String>,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: PanelPosition::Top,
            height: 36,
            modules_left: vec!["logo".into(), "workspaces".into(), "window-title".into()],
            modules_center: vec!["clock".into()],
            modules_right: vec![
                "tray".into(),
                "network".into(),
                "volume".into(),
                "battery".into(),
                "power".into(),
            ],
        }
    }
}

/// Привязка клавиш к действию.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub keys: Shortcut,
    #[serde(flatten)]
    pub action: Action,
}

impl Binding {
    pub fn new(keys: &str, action: Action) -> Self {
        Self {
            keys: keys
                .parse()
                .expect("привязка по умолчанию должна разбираться"),
            action,
        }
    }
}

/// Весь конфиг среды.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: Theme,
    pub input: InputConfig,
    pub layout: LayoutConfig,
    pub animations: AnimationConfig,
    pub panel: PanelConfig,
    /// Команды, запускаемые при входе в сеанс.
    pub autostart: Vec<String>,
    /// Привязки клавиш. Заданные пользователем полностью заменяют набор по
    /// умолчанию — иначе от ненужной привязки невозможно избавиться.
    #[serde(rename = "keybind")]
    pub keybinds: Vec<Binding>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            input: InputConfig::default(),
            layout: LayoutConfig::default(),
            animations: AnimationConfig::default(),
            panel: PanelConfig::default(),
            autostart: Vec::new(),
            keybinds: default_keybinds(),
        }
    }
}

impl Config {
    /// Ищет действие, привязанное к сочетанию клавиш.
    pub fn action_for(&self, shortcut: &Shortcut) -> Option<&Action> {
        self.keybinds
            .iter()
            .find(|b| &b.keys == shortcut)
            .map(|b| &b.action)
    }

    /// Проверяет конфиг на подозрительные места.
    ///
    /// Возвращает список предупреждений: жёстко падать из-за спорной настройки
    /// нельзя — пользователь останется без рабочего стола, — но и молчать о
    /// том, что привязка не сработает, тоже нечестно.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Дубликаты: сработает только первая привязка.
        let mut seen: Vec<&Shortcut> = Vec::new();
        for binding in &self.keybinds {
            if seen.contains(&&binding.keys) {
                warnings.push(format!(
                    "сочетание {} привязано несколько раз, сработает только первое",
                    binding.keys
                ));
            } else {
                seen.push(&binding.keys);
            }
        }

        for binding in &self.keybinds {
            if let Action::Spawn { command } = &binding.action {
                if binding.action.command_line().is_none() {
                    warnings.push(format!(
                        "привязка {}: команду {command:?} не удалось разобрать",
                        binding.keys
                    ));
                }
            }
            if let Action::Workspace { index } | Action::MoveToWorkspace { index } = binding.action
            {
                if index == 0 || index > self.layout.workspaces {
                    warnings.push(format!(
                        "привязка {}: рабочего стола {index} не существует (их {})",
                        binding.keys, self.layout.workspaces
                    ));
                }
            }
        }

        if self.layout.workspaces == 0 {
            warnings.push("рабочих столов должно быть хотя бы один".into());
        }
        if !(0.1..=0.9).contains(&self.layout.master_ratio) {
            warnings.push(format!(
                "master_ratio = {} вне разумного диапазона 0.1..0.9",
                self.layout.master_ratio
            ));
        }
        if !(-1.0..=1.0).contains(&self.input.pointer_accel) {
            warnings.push(format!(
                "pointer_accel = {} вне диапазона -1..1",
                self.input.pointer_accel
            ));
        }
        if self.panel.enabled && self.panel.height == 0 {
            warnings.push("высота панели 0 — панель не будет видна".into());
        }
        if self.theme.motion.scale > 5.0 {
            warnings.push(format!(
                "множитель анимаций {} сделает среду мучительно медленной",
                self.theme.motion.scale
            ));
        }

        warnings
    }
}

/// Привязки клавиш по умолчанию.
///
/// Логика набора: `Super` — всё, что относится к среде; `Super+Shift` — то же
/// действие, но применённое к окну. Никаких сочетаний без модификаторов, кроме
/// `Print`, — иначе среда будет перехватывать клавиши у приложений.
pub fn default_keybinds() -> Vec<Binding> {
    let mut binds = vec![
        Binding::new(
            "Super+Return",
            Action::Spawn {
                command: "foot".into(),
            },
        ),
        Binding::new(
            "Super+E",
            Action::Spawn {
                command: "hype-files".into(),
            },
        ),
        Binding::new(
            "Super+I",
            Action::Spawn {
                command: "hype-settings".into(),
            },
        ),
        Binding::new("Super+Q", Action::CloseWindow),
        Binding::new("Super+F", Action::ToggleFullscreen),
        Binding::new("Super+M", Action::ToggleMaximized),
        Binding::new("Super+V", Action::ToggleFloating),
        Binding::new("Super+Space", Action::ToggleLauncher),
        Binding::new("Super+Tab", Action::ToggleOverview),
        Binding::new("Super+Shift+R", Action::ReloadConfig),
        Binding::new("Super+Shift+E", Action::Quit),
        Binding::new(
            "Print",
            Action::Screenshot {
                target: ScreenshotTarget::Screen,
            },
        ),
        Binding::new(
            "Shift+Print",
            Action::Screenshot {
                target: ScreenshotTarget::Region,
            },
        ),
    ];

    // Стрелки и hjkl делают одно и то же: и привычка из vim, и привычка из
    // обычных сред должны работать без правки конфига.
    for (direction, arrow, letter) in [
        (Direction::Left, "Left", "H"),
        (Direction::Down, "Down", "J"),
        (Direction::Up, "Up", "K"),
        (Direction::Right, "Right", "L"),
    ] {
        for key in [arrow, letter] {
            binds.push(Binding::new(
                &format!("Super+{key}"),
                Action::FocusDirection { direction },
            ));
            binds.push(Binding::new(
                &format!("Super+Shift+{key}"),
                Action::MoveWindow { direction },
            ));
            binds.push(Binding::new(
                &format!("Super+Ctrl+{key}"),
                Action::ResizeWindow {
                    direction,
                    delta: 40,
                },
            ));
        }
    }

    for index in 1..=9u8 {
        binds.push(Binding::new(
            &format!("Super+{index}"),
            Action::Workspace { index },
        ));
        binds.push(Binding::new(
            &format!("Super+Shift+{index}"),
            Action::MoveToWorkspace { index },
        ));
    }

    binds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let warnings = Config::default().validate();
        assert!(warnings.is_empty(), "предупреждения: {warnings:?}");
    }

    #[test]
    fn default_keybinds_have_no_duplicates() {
        let binds = default_keybinds();
        let mut seen = Vec::new();
        for b in &binds {
            assert!(
                !seen.contains(&b.keys),
                "сочетание {} задано дважды",
                b.keys
            );
            seen.push(b.keys.clone());
        }
    }

    #[test]
    fn config_survives_a_toml_round_trip() {
        let config = Config::default();
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn a_partial_file_keeps_the_rest_of_the_defaults() {
        let config: Config = toml::from_str(
            r##"
            [layout]
            gaps_inner = 20.0

            [theme]
            accent = "#ff0066"
        "##,
        )
        .unwrap();

        assert_eq!(config.layout.gaps_inner, 20.0);
        // Не заданное в файле осталось стандартным.
        assert_eq!(config.layout.gaps_outer, LayoutConfig::default().gaps_outer);
        assert_eq!(config.input, InputConfig::default());
        assert_eq!(config.keybinds, default_keybinds());
    }

    #[test]
    fn user_keybinds_replace_the_defaults_entirely() {
        let config: Config = toml::from_str(
            r#"
            [[keybind]]
            keys = "Super+T"
            do = "spawn"
            command = "alacritty"
        "#,
        )
        .unwrap();

        assert_eq!(config.keybinds.len(), 1);
        assert_eq!(
            config.action_for(&"Super+T".parse().unwrap()),
            Some(&Action::Spawn {
                command: "alacritty".into()
            })
        );
        assert_eq!(config.action_for(&"Super+Q".parse().unwrap()), None);
    }

    #[test]
    fn a_bad_shortcut_fails_the_whole_parse() {
        let err = toml::from_str::<Config>(
            r#"
            [[keybind]]
            keys = "Super+Nonsense"
            do = "close-window"
        "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Nonsense"), "{err}");
    }

    #[test]
    fn validation_spots_duplicate_bindings() {
        let config = Config {
            keybinds: vec![
                Binding::new("Super+Q", Action::CloseWindow),
                Binding::new("Super+Q", Action::Quit),
            ],
            ..Config::default()
        };
        let warnings = config.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Super+Q"));
    }

    #[test]
    fn validation_spots_a_workspace_that_does_not_exist() {
        let mut config = Config::default();
        config.layout.workspaces = 4;
        let warnings = config.validate();
        assert!(
            warnings.iter().any(|w| w.contains("рабочего стола 9")),
            "{warnings:?}"
        );
    }

    #[test]
    fn validation_spots_a_broken_spawn_command() {
        let config = Config {
            keybinds: vec![Binding::new(
                "Super+T",
                Action::Spawn {
                    command: "foot -e 'htop".into(),
                },
            )],
            ..Config::default()
        };
        assert!(config.validate()[0].contains("разобрать"));
    }

    #[test]
    fn validation_spots_out_of_range_numbers() {
        let mut config = Config::default();
        config.layout.master_ratio = 1.5;
        config.input.pointer_accel = 9.0;
        config.panel.height = 0;
        let warnings = config.validate();
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn motion_scale_shrinks_timed_animations() {
        let spec = AnimSpec::timed(200, Easing::Linear);
        let Curve::Timed { duration, .. } = spec.curve_scaled(0.5) else {
            panic!("ожидалась кривая с длительностью");
        };
        assert_eq!(duration, Duration::from_millis(100));
    }

    #[test]
    fn a_disabled_animation_has_zero_duration() {
        let spec = AnimSpec {
            enabled: false,
            curve: Curve::timed(200, Easing::Linear),
        };
        let Curve::Timed { duration, .. } = spec.curve_scaled(1.0) else {
            panic!("выключенная анимация должна становиться мгновенной");
        };
        assert_eq!(duration, Duration::ZERO);
    }

    #[test]
    fn zero_motion_scale_disables_even_springs() {
        let spec = AnimSpec::spring(Spring::BOUNCY);
        assert!(matches!(
            spec.curve_scaled(0.0),
            Curve::Timed {
                duration: Duration::ZERO,
                ..
            }
        ));
    }

    #[test]
    fn slowing_a_spring_keeps_its_character_but_takes_longer() {
        let spec = AnimSpec::spring(Spring::SMOOTH);
        let Curve::Spring(slow) = spec.curve_scaled(2.0) else {
            panic!("ожидалась пружина");
        };
        assert_eq!(slow.damping_ratio, Spring::SMOOTH.damping_ratio);
        assert!(slow.duration(0.0, 1.0, 0.0) > Spring::SMOOTH.duration(0.0, 1.0, 0.0));
    }

    #[test]
    fn action_lookup_finds_the_binding() {
        let config = Config::default();
        assert_eq!(
            config.action_for(&"Super+Q".parse().unwrap()),
            Some(&Action::CloseWindow)
        );
        assert_eq!(config.action_for(&"Super+Z".parse().unwrap()), None);
    }

    #[test]
    fn both_vim_and_arrow_navigation_work_out_of_the_box() {
        let config = Config::default();
        let by_arrow = config.action_for(&"Super+Left".parse().unwrap());
        let by_letter = config.action_for(&"Super+H".parse().unwrap());
        assert_eq!(by_arrow, by_letter);
        assert!(by_arrow.is_some());
    }
}
