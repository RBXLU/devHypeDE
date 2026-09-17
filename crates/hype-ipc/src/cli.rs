//! Разбор аргументов `hypectl`.
//!
//! Обходимся без внешнего разборщика: набор команд маленький, а зависимость
//! ради десятка строк утяжелила бы крейт, который линкуется в композитор.

use hype_config::{Action, Direction, ScreenshotTarget};

use crate::{EventKind, Request};

/// Что попросили сделать.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// Обычный запрос к композитору.
    Request(Request),
    /// Подписка на события.
    Subscribe(Vec<EventKind>),
    Help,
}

/// Текст справки.
pub const HELP: &str = "\
hypectl — управление композитором HypeDE

Использование:
  hypectl ping                     проверить, что композитор отвечает
  hypectl version                  версия композитора
  hypectl state                    полное состояние среды (JSON)
  hypectl windows                  список окон
  hypectl workspaces               список рабочих столов
  hypectl config                   текущие настройки
  hypectl dispatch <действие>      выполнить действие среды
  hypectl subscribe [виды]         следить за событиями

Действия:
  spawn <команда>                  запустить программу
  close                            закрыть окно в фокусе
  fullscreen | maximize | float    переключить состояние окна
  focus <left|right|up|down>       перевести фокус
  move <left|right|up|down>        переставить окно
  workspace <номер>                перейти на рабочий стол
  move-to <номер>                  перенести окно на рабочий стол
  next | prev                      соседний рабочий стол
  launcher | overview              поиск приложений, обзор окон
  screenshot [screen|window|region] снимок экрана
  reload                           перечитать настройки
  quit                             завершить сеанс

Виды событий: window, workspace, output, focus, theme.
Без указания подписка идёт на все.

Примеры:
  hypectl dispatch workspace 3
  hypectl dispatch spawn foot -e htop
  hypectl subscribe window focus
";

/// Ошибка разбора командной строки.
#[derive(Debug, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Разбирает аргументы (без имени программы).
pub fn parse(args: &[String]) -> Result<Command, ParseError> {
    let Some(first) = args.first() else {
        return Ok(Command::Help);
    };

    match first.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "ping" => Ok(Command::Request(Request::Ping)),
        "version" | "--version" => Ok(Command::Request(Request::Version)),
        "state" => Ok(Command::Request(Request::GetState)),
        "windows" => Ok(Command::Request(Request::ListWindows)),
        "workspaces" => Ok(Command::Request(Request::ListWorkspaces)),
        "config" => Ok(Command::Request(Request::GetConfig)),
        "dispatch" => {
            parse_action(&args[1..]).map(|action| Command::Request(Request::Dispatch { action }))
        }
        "subscribe" => parse_events(&args[1..]).map(Command::Subscribe),
        other => Err(ParseError(format!(
            "неизвестная команда «{other}». Список: hypectl help"
        ))),
    }
}

fn parse_action(args: &[String]) -> Result<Action, ParseError> {
    let Some(name) = args.first() else {
        return Err(ParseError("после dispatch нужно указать действие".into()));
    };
    let rest = &args[1..];

    let index = |what: &str| -> Result<u8, ParseError> {
        let value = rest
            .first()
            .ok_or_else(|| ParseError(format!("{what}: нужен номер рабочего стола")))?;
        value
            .parse::<u8>()
            .map_err(|_| ParseError(format!("«{value}» не похоже на номер рабочего стола")))
    };

    match name.as_str() {
        "spawn" => {
            if rest.is_empty() {
                return Err(ParseError("spawn: нужна команда для запуска".into()));
            }
            // Остаток строки склеивается обратно: так `hypectl dispatch spawn
            // foot -e htop` работает без кавычек.
            Ok(Action::Spawn {
                command: rest.join(" "),
            })
        }
        "close" => Ok(Action::CloseWindow),
        "fullscreen" => Ok(Action::ToggleFullscreen),
        "maximize" => Ok(Action::ToggleMaximized),
        "float" => Ok(Action::ToggleFloating),
        "focus" => Ok(Action::FocusDirection {
            direction: parse_direction(rest.first())?,
        }),
        "move" => Ok(Action::MoveWindow {
            direction: parse_direction(rest.first())?,
        }),
        "workspace" => Ok(Action::Workspace {
            index: index("workspace")?,
        }),
        "move-to" => Ok(Action::MoveToWorkspace {
            index: index("move-to")?,
        }),
        "next" => Ok(Action::NextWorkspace),
        "prev" => Ok(Action::PrevWorkspace),
        "launcher" => Ok(Action::ToggleLauncher),
        "overview" => Ok(Action::ToggleOverview),
        "screenshot" => Ok(Action::Screenshot {
            target: match rest.first().map(String::as_str) {
                None | Some("screen") => ScreenshotTarget::Screen,
                Some("window") => ScreenshotTarget::Window,
                Some("region") | Some("area") => ScreenshotTarget::Region,
                Some(other) => {
                    return Err(ParseError(format!(
                        "screenshot: «{other}» — ожидалось screen, window или region"
                    )))
                }
            },
        }),
        "reload" => Ok(Action::ReloadConfig),
        "quit" => Ok(Action::Quit),
        other => Err(ParseError(format!(
            "неизвестное действие «{other}». Список: hypectl help"
        ))),
    }
}

fn parse_direction(value: Option<&String>) -> Result<Direction, ParseError> {
    match value.map(String::as_str) {
        Some("left") | Some("l") => Ok(Direction::Left),
        Some("right") | Some("r") => Ok(Direction::Right),
        Some("up") | Some("u") => Ok(Direction::Up),
        Some("down") | Some("d") => Ok(Direction::Down),
        Some(other) => Err(ParseError(format!(
            "«{other}» — ожидалось left, right, up или down"
        ))),
        None => Err(ParseError(
            "нужно направление: left, right, up или down".into(),
        )),
    }
}

fn parse_events(args: &[String]) -> Result<Vec<EventKind>, ParseError> {
    if args.is_empty() {
        return Ok(vec![
            EventKind::Window,
            EventKind::Workspace,
            EventKind::Output,
            EventKind::Focus,
            EventKind::Theme,
        ]);
    }

    args.iter()
        .map(|arg| match arg.as_str() {
            "window" => Ok(EventKind::Window),
            "workspace" => Ok(EventKind::Workspace),
            "output" => Ok(EventKind::Output),
            "focus" => Ok(EventKind::Focus),
            "theme" => Ok(EventKind::Theme),
            other => Err(ParseError(format!(
                "неизвестный вид событий «{other}». Есть: window, workspace, output, focus, theme"
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(input: &[&str]) -> Vec<String> {
        input.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_shows_help() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Command::Help);
    }

    #[test]
    fn simple_queries_map_to_requests() {
        assert_eq!(
            parse(&args(&["ping"])).unwrap(),
            Command::Request(Request::Ping)
        );
        assert_eq!(
            parse(&args(&["windows"])).unwrap(),
            Command::Request(Request::ListWindows)
        );
    }

    #[test]
    fn dispatch_parses_every_action_shape() {
        let action = |input: &[&str]| match parse(&args(input)).unwrap() {
            Command::Request(Request::Dispatch { action }) => action,
            other => panic!("ожидалось действие, получено {other:?}"),
        };

        assert_eq!(action(&["dispatch", "close"]), Action::CloseWindow);
        assert_eq!(
            action(&["dispatch", "workspace", "3"]),
            Action::Workspace { index: 3 }
        );
        assert_eq!(
            action(&["dispatch", "focus", "left"]),
            Action::FocusDirection {
                direction: Direction::Left
            }
        );
        assert_eq!(
            action(&["dispatch", "screenshot"]),
            Action::Screenshot {
                target: ScreenshotTarget::Screen
            }
        );
    }

    #[test]
    fn spawn_keeps_the_whole_command_line() {
        let Command::Request(Request::Dispatch { action }) =
            parse(&args(&["dispatch", "spawn", "foot", "-e", "htop"])).unwrap()
        else {
            panic!("ожидался запуск программы");
        };
        assert_eq!(
            action,
            Action::Spawn {
                command: "foot -e htop".into()
            }
        );
    }

    #[test]
    fn direction_shortcuts_work() {
        let action = |input: &[&str]| match parse(&args(input)).unwrap() {
            Command::Request(Request::Dispatch { action }) => action,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            action(&["dispatch", "move", "r"]),
            Action::MoveWindow {
                direction: Direction::Right
            }
        );
    }

    #[test]
    fn subscribe_without_arguments_takes_everything() {
        let Command::Subscribe(kinds) = parse(&args(&["subscribe"])).unwrap() else {
            panic!("ожидалась подписка");
        };
        assert_eq!(kinds.len(), 5);
    }

    #[test]
    fn subscribe_filters_by_kind() {
        let Command::Subscribe(kinds) = parse(&args(&["subscribe", "window", "focus"])).unwrap()
        else {
            panic!("ожидалась подписка");
        };
        assert_eq!(kinds, vec![EventKind::Window, EventKind::Focus]);
    }

    #[test]
    fn mistakes_are_explained_rather_than_swallowed() {
        assert!(parse(&args(&["чепуха"]))
            .unwrap_err()
            .0
            .contains("неизвестная команда"));
        assert!(parse(&args(&["dispatch"]))
            .unwrap_err()
            .0
            .contains("нужно указать"));
        assert!(parse(&args(&["dispatch", "workspace"]))
            .unwrap_err()
            .0
            .contains("номер"));
        assert!(parse(&args(&["dispatch", "workspace", "много"]))
            .unwrap_err()
            .0
            .contains("не похоже на номер"));
        assert!(parse(&args(&["dispatch", "focus", "вбок"]))
            .unwrap_err()
            .0
            .contains("ожидалось left"));
        assert!(parse(&args(&["subscribe", "погода"]))
            .unwrap_err()
            .0
            .contains("неизвестный вид"));
        assert!(parse(&args(&["dispatch", "spawn"]))
            .unwrap_err()
            .0
            .contains("нужна команда"));
    }

    #[test]
    fn the_help_text_lists_every_command_parse_accepts() {
        for command in [
            "ping",
            "version",
            "state",
            "windows",
            "workspaces",
            "dispatch",
            "subscribe",
        ] {
            assert!(HELP.contains(command), "в справке нет команды {command}");
        }
    }
}
