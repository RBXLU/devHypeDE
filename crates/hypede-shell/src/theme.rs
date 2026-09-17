//! Подключение темы среды к окнам оболочки.

use gtk4::gdk;
use gtk4::CssProvider;

/// Подключает CSS, выгруженный композитором.
///
/// Если файла нет, окно берёт системную тему: оболочку можно запустить и вне
/// сеанса HypeDE, например для отладки.
pub fn load() {
    let Some(path) = hype_config::paths::generated_css_file() else {
        return;
    };
    if !path.exists() {
        return;
    }

    let provider = CssProvider::new();
    provider.load_from_file(&gtk4::gio::File::for_path(&path));

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
