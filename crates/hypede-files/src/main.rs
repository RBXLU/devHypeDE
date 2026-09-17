//! Файловый менеджер HypeDE.

use gtk4::prelude::*;
use gtk4::Application;

fn main() -> gtk4::glib::ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let app = Application::builder()
        .application_id(hypede_files::APP_ID)
        .build();

    app.connect_activate(hypede_files::ui::build);
    app.run()
}
