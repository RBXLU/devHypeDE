//! Сообщения протокола управления.
//!
//! Формат — JSON, по одному сообщению на строку. Выбор намеренный: такой поток
//! читается человеком в терминале, разбирается из shell через `jq` и не требует
//! генератора кода. Пропускная способность тут не нужна — за кадр проходит
//! десяток сообщений, а не десяток тысяч.

use hype_config::{Action, Config};
use serde::{Deserialize, Serialize};

/// Запрос к композитору.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// Проверка, что композитор жив.
    Ping,
    /// Версия композитора.
    Version,
    /// Полное состояние: мониторы, рабочие столы, окна.
    GetState,
    /// Только список окон.
    ListWindows,
    /// Только список рабочих столов.
    ListWorkspaces,
    /// Текущие настройки.
    GetConfig,
    /// Выполнить действие среды — то же, что нажать горячую клавишу.
    Dispatch { action: Action },
    /// Применить новые настройки без перезапуска.
    ApplyConfig { config: Box<Config> },
    /// Подписаться на события. После этого соединение работает только на
    /// приём: композитор шлёт в него события до разрыва.
    Subscribe { events: Vec<EventKind> },
}

/// Ответ композитора.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "kebab-case")]
pub enum Response {
    /// Команда принята, возвращать нечего.
    Ok,
    Pong,
    Version {
        version: String,
    },
    State(Box<State>),
    Windows {
        windows: Vec<WindowInfo>,
    },
    Workspaces {
        workspaces: Vec<WorkspaceInfo>,
    },
    Config {
        config: Box<Config>,
    },
    /// Запрос не выполнен. Текст предназначен человеку.
    Error {
        message: String,
    },
}

impl Response {
    /// Ошибка с сообщением.
    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
        }
    }

    /// Является ли ответ ошибкой.
    pub fn is_error(&self) -> bool {
        matches!(self, Response::Error { .. })
    }
}

/// Вид события — им подписчик фильтрует поток.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    Window,
    Workspace,
    Output,
    Focus,
    Theme,
    /// Просьбы к оболочке: открыть поиск приложений, показать обзор окон.
    Shell,
}

/// Событие, происходящее в среде.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    WindowOpened {
        window: WindowInfo,
    },
    WindowClosed {
        id: u64,
    },
    WindowChanged {
        window: WindowInfo,
    },
    /// Фокус перешёл к окну; `None` означает, что фокуса нет ни у кого.
    FocusChanged {
        id: Option<u64>,
    },
    WorkspaceChanged {
        index: u8,
    },
    OutputAdded {
        output: OutputInfo,
    },
    OutputRemoved {
        name: String,
    },
    ThemeChanged,
    /// Композитор просит оболочку что-то показать или спрятать.
    ///
    /// Оболочка держит эти окна у себя: если бы композитор запускал их
    /// отдельными процессами, каждое нажатие открывало бы ещё одно поверх
    /// прежнего, а закрыть их было бы нечем.
    ShellRequest {
        request: ShellRequest,
    },
}

/// Что композитор просит показать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellRequest {
    /// Показать поиск приложений, а если он открыт — закрыть.
    ToggleLauncher,
    /// Показать обзор окон.
    ToggleOverview,
}

impl Event {
    /// К какому виду относится событие — по нему работает подписка.
    pub fn kind(&self) -> EventKind {
        match self {
            Event::WindowOpened { .. }
            | Event::WindowClosed { .. }
            | Event::WindowChanged { .. } => EventKind::Window,
            Event::FocusChanged { .. } => EventKind::Focus,
            Event::WorkspaceChanged { .. } => EventKind::Workspace,
            Event::OutputAdded { .. } | Event::OutputRemoved { .. } => EventKind::Output,
            Event::ThemeChanged => EventKind::Theme,
            Event::ShellRequest { .. } => EventKind::Shell,
        }
    }
}

/// То, что течёт по соединению от композитора к клиенту.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outgoing {
    Response(Response),
    Event(Event),
}

/// Окно.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub title: String,
    /// Идентификатор приложения (`app_id` в xdg-shell).
    pub app_id: String,
    pub workspace: u8,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
    pub floating: bool,
    pub fullscreen: bool,
}

/// Рабочий стол.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub index: u8,
    pub name: String,
    pub windows: usize,
    pub active: bool,
    /// Монитор, на котором показан этот рабочий стол.
    pub output: String,
}

/// Монитор.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub scale: f64,
    pub x: i32,
    pub y: i32,
}

impl OutputInfo {
    /// Частота обновления в герцах.
    pub fn refresh_hz(&self) -> f64 {
        self.refresh_mhz as f64 / 1000.0
    }
}

/// Состояние среды целиком.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: String,
    pub outputs: Vec<OutputInfo>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub windows: Vec<WindowInfo>,
    pub focused_window: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_through_json() {
        let requests = vec![
            Request::Ping,
            Request::GetState,
            Request::Dispatch {
                action: Action::CloseWindow,
            },
            Request::Subscribe {
                events: vec![EventKind::Window, EventKind::Focus],
            },
        ];

        for request in requests {
            let text = serde_json::to_string(&request).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
        }
    }

    #[test]
    fn request_json_is_readable_from_a_shell() {
        // Форма сообщения — часть публичного интерфейса: по ней люди пишут
        // скрипты, поэтому она закреплена тестом.
        let text = serde_json::to_string(&Request::Dispatch {
            action: Action::Workspace { index: 3 },
        })
        .unwrap();
        assert_eq!(
            text,
            r#"{"request":"dispatch","action":{"do":"workspace","index":3}}"#
        );
    }

    #[test]
    fn responses_round_trip_through_json() {
        let response = Response::Windows {
            windows: vec![WindowInfo {
                id: 1,
                title: "Терминал".into(),
                app_id: "foot".into(),
                workspace: 1,
                x: 0,
                y: 0,
                width: 800,
                height: 600,
                focused: true,
                floating: false,
                fullscreen: false,
            }],
        };
        let text = serde_json::to_string(&response).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), response);
    }

    #[test]
    fn events_report_their_kind() {
        assert_eq!(Event::WindowClosed { id: 1 }.kind(), EventKind::Window);
        assert_eq!(Event::FocusChanged { id: None }.kind(), EventKind::Focus);
        assert_eq!(Event::ThemeChanged.kind(), EventKind::Theme);
    }

    #[test]
    fn shell_requests_are_their_own_kind() {
        let event = Event::ShellRequest {
            request: ShellRequest::ToggleLauncher,
        };
        assert_eq!(event.kind(), EventKind::Shell);

        let text = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&text).unwrap(), event);
    }

    #[test]
    fn outgoing_keeps_responses_and_events_apart() {
        let response = Outgoing::Response(Response::Pong);
        let event = Outgoing::Event(Event::WorkspaceChanged { index: 2 });

        let response_text = serde_json::to_string(&response).unwrap();
        let event_text = serde_json::to_string(&event).unwrap();

        assert_eq!(
            serde_json::from_str::<Outgoing>(&response_text).unwrap(),
            response
        );
        assert_eq!(
            serde_json::from_str::<Outgoing>(&event_text).unwrap(),
            event
        );
    }

    #[test]
    fn error_responses_are_recognisable() {
        assert!(Response::error("окна не существует").is_error());
        assert!(!Response::Ok.is_error());
    }

    #[test]
    fn refresh_rate_converts_from_millihertz() {
        let output = OutputInfo {
            name: "DP-1".into(),
            description: "Монитор".into(),
            width: 2560,
            height: 1440,
            refresh_mhz: 165_000,
            scale: 1.0,
            x: 0,
            y: 0,
        };
        assert_eq!(output.refresh_hz(), 165.0);
    }
}
