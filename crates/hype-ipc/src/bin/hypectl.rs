//! Управление композитором HypeDE из терминала.

use std::process::ExitCode;

use hype_ipc::cli::{parse, Command, HELP};
use hype_ipc::{Client, Response};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = match parse(&args) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("hypectl: {err}");
            return ExitCode::from(2);
        }
    };

    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("hypectl: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), String> {
    if matches!(command, Command::Help) {
        print!("{HELP}");
        return Ok(());
    }

    let mut client = Client::connect_default().map_err(|err| err.to_string())?;

    match command {
        Command::Help => unreachable!("обработано выше"),

        Command::Request(request) => {
            let response = client.request(request).map_err(|err| err.to_string())?;
            if let Response::Error { message } = &response {
                return Err(message.clone());
            }
            println!("{}", render(&response));
            Ok(())
        }

        Command::Subscribe(kinds) => {
            // Поток событий бесконечен: выход — Ctrl+C или остановка
            // композитора. Каждое событие печатается строкой JSON, чтобы его
            // можно было читать конвейером.
            for event in client.subscribe(kinds).map_err(|err| err.to_string())? {
                let event = event.map_err(|err| err.to_string())?;
                println!(
                    "{}",
                    serde_json::to_string(&event).unwrap_or_else(|_| "{}".into())
                );
            }
            Ok(())
        }
    }
}

/// Готовит ответ к показу человеку.
fn render(response: &Response) -> String {
    match response {
        Response::Ok => "готово".to_string(),
        Response::Pong => "композитор отвечает".to_string(),
        Response::Version { version } => format!("HypeDE {version}"),

        Response::Windows { windows } => {
            if windows.is_empty() {
                return "окон нет".to_string();
            }
            windows
                .iter()
                .map(|w| {
                    format!(
                        "{:>4}  {}{:<16} {:>4}×{:<4} стол {}  {}",
                        w.id,
                        if w.focused { "▸ " } else { "  " },
                        w.app_id,
                        w.width,
                        w.height,
                        w.workspace,
                        w.title
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        Response::Workspaces { workspaces } => workspaces
            .iter()
            .map(|w| {
                format!(
                    "{}{}  окон: {}",
                    if w.active { "▸ " } else { "  " },
                    w.name,
                    w.windows
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),

        // Состояние и настройки отдаются как есть: их читают скриптами.
        Response::State(state) => {
            serde_json::to_string_pretty(state).unwrap_or_else(|err| format!("{err}"))
        }
        Response::Config { config } => {
            toml::to_string_pretty(config).unwrap_or_else(|err| format!("{err}"))
        }
        Response::Error { message } => format!("ошибка: {message}"),
    }
}
