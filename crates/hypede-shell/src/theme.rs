//! Подключение темы среды к окнам.

use gtk4::gdk;
use gtk4::CssProvider;

/// Подключает CSS темы и значки среды.
///
/// Если файла темы нет, окно берёт системную: любое приложение HypeDE можно
/// запустить и вне сеанса среды.
pub fn load() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    load_icons(&display);
    load_css(&display);
}

/// Добавляет каталоги со значками среды в поиск GTK.
///
/// Без этого собственные значки приложений находятся только после установки
/// пакетом, и при запуске из сборки вместо них показывается заглушка.
fn load_icons(display: &gdk::Display) {
    let theme = gtk4::IconTheme::for_display(display);
    for dir in hype_config::paths::asset_dirs() {
        let icons = dir.join("icons");
        if icons.is_dir() {
            tracing::debug!("значки среды: {}", icons.display());
            theme.add_search_path(&icons);
        }
    }
    tracing::debug!(
        "собственный значок найден: {}",
        theme.has_icon("dev.hypede.Files")
    );
}

fn load_css(display: &gdk::Display) {
    let Some(path) = hype_config::paths::generated_css_file() else {
        return;
    };
    if !path.exists() {
        return;
    }

    let provider = CssProvider::new();
    provider.load_from_file(&gtk4::gio::File::for_path(&path));

    gtk4::style_context_add_provider_for_display(
        display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
