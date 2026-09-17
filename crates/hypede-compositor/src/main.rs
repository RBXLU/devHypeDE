//! Точка входа композитора HypeDE.

use std::process::ExitCode;

use hype_ipc::Listener;
use hypede_compositor::ipc::{IpcServer, PendingRequest};
use hypede_compositor::state::{HypeState, LoopData};
use hypede_compositor::winit;
use smithay::reexports::calloop::channel;
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use tracing::{error, info, warn};

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

    winit::init(&mut event_loop, &mut data)?;
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

    event_loop.run(None, &mut data, |_| {})?;
    Ok(())
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
