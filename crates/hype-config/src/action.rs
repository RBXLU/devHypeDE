//! Действия, которые умеет выполнять композитор.
//!
//! Один и тот же тип используется и в горячих клавишах, и в IPC: то, что можно
//! повесить на клавишу, можно вызвать из скрипта, и наоборот. Это избавляет от
//! двух расходящихся списков команд — болезни, которой страдает не одна среда.

use serde::{Deserialize, Serialize};

/// Направление на экране.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Противоположное направление.
    pub fn opposite(self) -> Self {
        match self {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    /// Единичный вектор `(dx, dy)` в экранных координатах: ось Y растёт вниз.
    pub fn vector(self) -> (f64, f64) {
        match self {
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
        }
    }
}

/// Что именно снять при скриншоте.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScreenshotTarget {
    Screen,
    Window,
    Region,
}

/// Действие среды.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "kebab-case")]
pub enum Action {
    /// Запустить программу. Команда разбирается по пробелам, без оболочки:
    /// так в конфиге не заводится случайный `rm -rf` через подстановку.
    Spawn {
        command: String,
    },

    CloseWindow,
    ToggleFullscreen,
    ToggleMaximized,
    ToggleFloating,

    /// Перевести фокус на соседнее окно.
    FocusDirection {
        direction: Direction,
    },
    /// Перевести фокус на окно по его номеру.
    ///
    /// Нужна полке: щелчок по значку запущенного приложения должен
    /// переключать на него, а не запускать второй экземпляр.
    FocusWindow {
        id: u64,
    },
    /// Переставить текущее окно.
    MoveWindow {
        direction: Direction,
    },
    /// Изменить размер текущего окна на `delta` логических пикселей.
    ResizeWindow {
        direction: Direction,
        delta: i32,
    },

    /// Перейти на рабочий стол по номеру, начиная с 1.
    Workspace {
        index: u8,
    },
    /// Перенести окно на рабочий стол и перейти туда.
    MoveToWorkspace {
        index: u8,
    },
    NextWorkspace,
    PrevWorkspace,

    /// Показать обзор всех окон.
    ToggleOverview,
    /// Показать поиск по приложениям.
    ToggleLauncher,

    Screenshot {
        target: ScreenshotTarget,
    },

    /// Перечитать конфигурацию с диска.
    ReloadConfig,
    /// Завершить сеанс.
    Quit,
}

impl Action {
    /// Короткое описание для подсказок и настроек.
    pub fn description(&self) -> String {
        match self {
            Action::Spawn { command } => format!("Запустить {command}"),
            Action::CloseWindow => "Закрыть окно".into(),
            Action::ToggleFullscreen => "Полный экран".into(),
            Action::ToggleMaximized => "Развернуть окно".into(),
            Action::ToggleFloating => "Плавающее окно".into(),
            Action::FocusDirection { direction } => {
                format!("Фокус {}", direction_word(*direction))
            }
            Action::FocusWindow { id } => format!("Фокус на окно {id}"),
            Action::MoveWindow { direction } => {
                format!("Переместить окно {}", direction_word(*direction))
            }
            Action::ResizeWindow { direction, delta } => {
                format!("Размер {} на {delta} px", direction_word(*direction))
            }
            Action::Workspace { index } => format!("Рабочий стол {index}"),
            Action::MoveToWorkspace { index } => format!("Окно на рабочий стол {index}"),
            Action::NextWorkspace => "Следующий рабочий стол".into(),
            Action::PrevWorkspace => "Предыдущий рабочий стол".into(),
            Action::ToggleOverview => "Обзор окон".into(),
            Action::ToggleLauncher => "Поиск приложений".into(),
            Action::Screenshot { target } => match target {
                ScreenshotTarget::Screen => "Снимок экрана".into(),
                ScreenshotTarget::Window => "Снимок окна".into(),
                ScreenshotTarget::Region => "Снимок области".into(),
            },
            Action::ReloadConfig => "Перечитать настройки".into(),
            Action::Quit => "Завершить сеанс".into(),
        }
    }

    /// Разбирает команду `Spawn` на программу и аргументы.
    ///
    /// Понимает кавычки: `spawn = "foot -e 'htop -d 5'"` даёт три аргумента.
    /// Возвращает `None`, если команда пуста или кавычка не закрыта.
    pub fn command_line(&self) -> Option<Vec<String>> {
        let Action::Spawn { command } = self else {
            return None;
        };
        split_command(command)
    }
}

fn direction_word(d: Direction) -> &'static str {
    match d {
        Direction::Left => "влево",
        Direction::Right => "вправо",
        Direction::Up => "вверх",
        Direction::Down => "вниз",
    }
}

/// Разбивает командную строку на аргументы с учётом одинарных и двойных кавычек.
fn split_command(input: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_current = false;

    for c in input.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                // Пустые кавычки — это тоже аргумент: `foo ""`.
                has_current = true;
            }
            None if c.is_whitespace() => {
                if has_current {
                    args.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            None => {
                current.push(c);
                has_current = true;
            }
        }
    }

    if quote.is_some() {
        return None;
    }
    if has_current {
        args.push(current);
    }

    (!args.is_empty()).then_some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_have_opposites_and_vectors() {
        assert_eq!(Direction::Left.opposite(), Direction::Right);
        assert_eq!(Direction::Up.opposite().opposite(), Direction::Up);
        // Ось Y на экране растёт вниз — «вверх» это отрицательный Y.
        assert_eq!(Direction::Up.vector(), (0.0, -1.0));
        assert_eq!(Direction::Right.vector(), (1.0, 0.0));
    }

    #[test]
    fn actions_round_trip_through_toml() {
        let actions = vec![
            Action::Spawn {
                command: "foot".into(),
            },
            Action::CloseWindow,
            Action::FocusDirection {
                direction: Direction::Left,
            },
            Action::ResizeWindow {
                direction: Direction::Right,
                delta: 40,
            },
            Action::Workspace { index: 3 },
            Action::Screenshot {
                target: ScreenshotTarget::Region,
            },
        ];

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Wrapper {
            action: Vec<Action>,
        }

        let wrapper = Wrapper { action: actions };
        let text = toml::to_string(&wrapper).unwrap();
        let back: Wrapper = toml::from_str(&text).unwrap();
        assert_eq!(wrapper, back);
    }

    #[test]
    fn action_tag_is_human_friendly_in_toml() {
        let text = toml::to_string(&Wrapper {
            action: Action::ToggleFullscreen,
        })
        .unwrap();
        assert!(text.contains("do = \"toggle-fullscreen\""), "{text}");
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Wrapper {
        action: Action,
    }

    #[test]
    fn splits_a_plain_command() {
        let action = Action::Spawn {
            command: "foot -e htop".into(),
        };
        assert_eq!(action.command_line().unwrap(), vec!["foot", "-e", "htop"]);
    }

    #[test]
    fn splits_quoted_arguments_as_one() {
        let action = Action::Spawn {
            command: "foot -e 'htop -d 5'".into(),
        };
        assert_eq!(
            action.command_line().unwrap(),
            vec!["foot", "-e", "htop -d 5"]
        );
    }

    #[test]
    fn collapses_repeated_whitespace() {
        let action = Action::Spawn {
            command: "  foot   -e    htop  ".into(),
        };
        assert_eq!(action.command_line().unwrap(), vec!["foot", "-e", "htop"]);
    }

    #[test]
    fn keeps_an_explicitly_empty_argument() {
        let action = Action::Spawn {
            command: "prog \"\" tail".into(),
        };
        assert_eq!(action.command_line().unwrap(), vec!["prog", "", "tail"]);
    }

    #[test]
    fn rejects_an_unterminated_quote() {
        let action = Action::Spawn {
            command: "foot -e 'htop".into(),
        };
        assert_eq!(action.command_line(), None);
    }

    #[test]
    fn empty_command_yields_nothing() {
        assert_eq!(
            Action::Spawn {
                command: "   ".into()
            }
            .command_line(),
            None
        );
    }

    #[test]
    fn non_spawn_actions_have_no_command_line() {
        assert_eq!(Action::Quit.command_line(), None);
    }

    #[test]
    fn focusing_a_window_round_trips() {
        let action = Action::FocusWindow { id: 42 };
        let text = toml::to_string(&Wrapper { action }).unwrap();
        assert!(text.contains("focus-window"), "{text}");
    }

    #[test]
    fn every_action_describes_itself() {
        assert_eq!(Action::CloseWindow.description(), "Закрыть окно");
        assert!(Action::Workspace { index: 2 }.description().contains('2'));
    }
}
