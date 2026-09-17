//! Оболочка HypeDE: панель или поиск приложений.

use gtk4::prelude::*;
use gtk4::Application;

fn main() -> gtk4::glib::ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let mode = std::env::args().nth(1).unwrap_or_default();

    match mode.as_str() {
        "--launcher" => run_launcher(),
        "--overview" => {
            // Обзор окон требует показа их содержимого уменьшенными копиями,
            // то есть работы на стороне композитора. Пока его нет, честнее
            // сказать об этом, чем открыть пустое окно.
            eprintln!("hype-shell: обзор окон ещё не реализован");
            gtk4::glib::ExitCode::FAILURE
        }
        "--help" | "-h" => {
            println!(
                "hype-shell — оболочка HypeDE\n\n\
                 Использование:\n  \
                 hype-shell              панель\n  \
                 hype-shell --launcher   поиск приложений\n"
            );
            gtk4::glib::ExitCode::SUCCESS
        }
        _ => run_panel(),
    }
}

fn run_panel() -> gtk4::glib::ExitCode {
    let config = hype_config::load()
        .map(|loaded| loaded.config)
        .unwrap_or_else(|err| {
            tracing::warn!("настройки не прочитаны, взяты значения по умолчанию: {err}");
            hype_config::Config::default()
        });

    // GTK берёт app_id окна из имени программы, а не из идентификатора
    // приложения. Композитор узнаёт панель именно по app_id, поэтому имя
    // задаётся явно.
    gtk4::glib::set_prgname(Some(hypede_shell::PANEL_APP_ID));

    let app = Application::builder()
        .application_id(hypede_shell::PANEL_APP_ID)
        .build();

    app.connect_activate(move |app| hypede_shell::panel::build(app, &config));
    app.run_with_args::<&str>(&[])
}

fn run_launcher() -> gtk4::glib::ExitCode {
    gtk4::glib::set_prgname(Some(hypede_shell::LAUNCHER_APP_ID));

    let app = Application::builder()
        .application_id(hypede_shell::LAUNCHER_APP_ID)
        .build();

    app.connect_activate(hypede_shell::launcher::build);
    app.run_with_args::<&str>(&[])
}
