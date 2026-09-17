//! Точка входа композитора HypeDE.

use std::process::ExitCode;

use std::time::Duration;

use hype_ipc::Listener;
use hypede_compositor::ipc::{IpcServer, PendingRequest};
use hypede_compositor::state::{HypeState, LoopData};
use hypede_compositor::{drm, winit};
use smithay::reexports::calloop::channel;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use tracing::{error, info, warn};

/// Как композитор выводит картинку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendKind {
    /// Окно внутри другого сеанса — режим разработки.
    Nested,
    /// Прямой доступ к видеокарте — настоящий сеанс.
    Drm,
}

impl BackendKind {
    /// Выбирает бэкенд по аргументам и окружению.
    ///
    /// Наличие `WAYLAND_DISPLAY` или `DISPLAY` означает, что вокруг уже есть
    /// сеанс: забирать у него видеокарту нельзя, значит запускаемся окном.
    /// Пустая консоль — значит, мы и есть сеанс.
    fn detect(args: &[String], env: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        for arg in args {
            match arg.as_str() {
                "--drm" | "--backend=drm" | "--tty" => return Ok(BackendKind::Drm),
                "--winit" | "--backend=winit" | "--nested" => return Ok(BackendKind::Nested),
                other if other.starts_with("--backend=") => {
                    return Err(format!(
                        "неизвестный бэкенд {:?}; доступны drm и winit",
                        &other["--backend=".len()..]
                    ))
                }
                _ => {}
            }
        }

        let has_session = env("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
            || env("DISPLAY").is_some_and(|v| !v.is_empty());

        Ok(if has_session {
            BackendKind::Nested
        } else {
            BackendKind::Drm
        })
    }
}

fn main() -> ExitCode {
    init_logging();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("композитор остановлен: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }

    let backend = BackendKind::detect(&args, |key| std::env::var(key).ok())
        .map_err(|message| anyhow::anyhow!(message))?;

    let loaded = hype_config::load()?;
    for warning in &loaded.warnings {
        warn!("настройки: {warning}");
    }
    match &loaded.source {
        Some(path) => info!("настройки прочитаны из {}", path.display()),
        None => info!("файла настроек нет, используются значения по умолчанию"),
    }

    // Тема выгружается в CSS до запуска приложений среды: панель должна
    // подняться уже в нужных цветах, а не перекраситься на глазах.
    match hype_config::export_theme_css(&loaded.config) {
        Ok(path) => info!("тема выгружена в {}", path.display()),
        Err(err) => warn!("не удалось выгрузить тему: {err}"),
    }

    let mut event_loop: EventLoop<'static, LoopData> = EventLoop::try_new()?;
    let display: Display<HypeState> = Display::new()?;
    let display_handle = display.handle();

    let autostart = loaded.config.autostart.clone();
    let state = HypeState::new(&mut event_loop, display, loaded.config);
    let mut data = LoopData {
        state,
        display_handle,
    };

    match backend {
        BackendKind::Nested => {
            info!("бэкенд: окно внутри текущего сеанса");
            winit::init(&mut event_loop, &mut data)?;
        }
        BackendKind::Drm => {
            info!("бэкенд: прямой доступ к видеокарте");
            drm::init(&mut event_loop, &mut data)?;
        }
    }

    start_control_socket(&mut event_loop, &mut data);

    info!(
        "HypeDE {} готов, сокет Wayland: {}",
        hypede_compositor::VERSION,
        data.state.socket_name.to_string_lossy()
    );

    // Панель — часть среды, а не пользовательская программа: без неё сеанс
    // выглядит недоделанным, поэтому она поднимается сама.
    if data.state.config.panel.enabled {
        data.state.dispatch(&hype_config::Action::Spawn {
            command: "hype-shell".into(),
        });
    }

    for command in &autostart {
        let action = hype_config::Action::Spawn {
            command: command.clone(),
        };
        data.state.dispatch(&action);
    }

    // Такт в 8 мс достаточно част для экрана в 120 Гц и почти ничего не стоит,
    // когда на экране ничего не движется: обработчик сразу выходит.
    event_loop.run(Some(Duration::from_millis(8)), &mut data, |data| {
        data.state.tick_drm();
    })?;
    Ok(())
}

/// Справка по аргументам.
const HELP: &str = "\
hypede-comp — композитор HypeDE

Использование:
  hypede-comp [--drm | --winit]

  --drm     прямой доступ к видеокарте: полноценный сеанс из консоли
  --winit   окно внутри текущего сеанса: режим разработки

Без аргументов бэкенд выбирается сам: если вокруг уже есть сеанс
(задан WAYLAND_DISPLAY или DISPLAY) — окном, иначе — на видеокарте.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn an_explicit_flag_wins_over_the_environment() {
        let inside_session = |key: &str| (key == "WAYLAND_DISPLAY").then(|| "wayland-0".to_string());

        assert_eq!(
            BackendKind::detect(&args(&["--drm"]), inside_session).unwrap(),
            BackendKind::Drm
        );
        assert_eq!(
            BackendKind::detect(&args(&["--winit"]), no_env).unwrap(),
            BackendKind::Nested
        );
    }

    #[test]
    fn a_running_session_means_nested() {
        for variable in ["WAYLAND_DISPLAY", "DISPLAY"] {
            let env = |key: &str| (key == variable).then(|| ":0".to_string());
            assert_eq!(
                BackendKind::detect(&[], env).unwrap(),
                BackendKind::Nested,
                "{variable}"
            );
        }
    }

    #[test]
    fn a_bare_console_means_the_graphics_card() {
        assert_eq!(BackendKind::detect(&[], no_env).unwrap(), BackendKind::Drm);
    }

    #[test]
    fn an_empty_variable_is_treated_as_unset() {
        let env = |key: &str| (key == "DISPLAY").then(String::new);
        assert_eq!(BackendKind::detect(&[], env).unwrap(), BackendKind::Drm);
    }

    #[test]
    fn an_unknown_backend_is_reported() {
        let err = BackendKind::detect(&args(&["--backend=vulkan"]), no_env).unwrap_err();
        assert!(err.contains("vulkan"), "{err}");
    }

    #[test]
    fn the_help_text_mentions_both_backends() {
        assert!(HELP.contains("--drm") && HELP.contains("--winit"));
    }
}

/// Поднимает сокет управления.
///
/// Без него среда работает, поэтому отказ — это предупреждение, а не остановка
/// сеанса: лучше потерять `hypectl`, чем рабочий стол.
fn start_control_socket(event_loop: &mut EventLoop<'static, LoopData>, data: &mut LoopData) {
    let listener = match Listener::bind_default() {
        Ok(listener) => listener,
        Err(err) => {
            warn!("управление по сокету недоступно: {err}");
            return;
        }
    };
    info!("сокет управления: {}", listener.path().display());

    let (sender, receiver) = channel::channel::<PendingRequest>();

    let inserted = event_loop
        .handle()
        .insert_source(receiver, |event, _, data| {
            if let channel::Event::Msg(pending) = event {
                let response = data.state.handle_ipc_request(pending.request);
                // Клиент мог отключиться, пока запрос ждал очереди.
                let _ = pending.reply.send(response);
            }
        })
        .is_ok();

    if !inserted {
        warn!("не удалось подключить канал управления к циклу событий");
        return;
    }

    data.state.ipc = Some(IpcServer::start(listener, sender));
}
