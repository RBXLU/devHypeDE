//! Особые роли окон среды.
//!
//! Панель и поиск приложений — обычные окна Wayland, но вести себя как обычные
//! окна они не должны: панель занимает отведённую полосу, лаунчер висит по
//! центру. Пока в композиторе нет протокола wlr-layer-shell, роль определяется
//! по идентификатору приложения.
//!
//! Это временное решение, и оно намеренно узкое: правило действует только для
//! собственных приложений среды, поэтому чужая программа не сможет объявить
//! себя панелью.

/// Как композитор обходится с окном.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Обычное окно: участвует в раскладке.
    Normal,
    /// Панель среды: занимает свою полосу, в раскладке не участвует.
    Panel,
    /// Поиск приложений: плавающее окно по центру экрана.
    Launcher,
}

/// Идентификатор панели HypeDE.
pub const PANEL_APP_ID: &str = "dev.hypede.Shell";
/// Идентификатор окна поиска приложений.
pub const LAUNCHER_APP_ID: &str = "dev.hypede.Launcher";

/// Определяет роль окна по идентификатору приложения.
pub fn role_for(app_id: &str) -> Role {
    match app_id {
        PANEL_APP_ID => Role::Panel,
        LAUNCHER_APP_ID => Role::Launcher,
        _ => Role::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_windows_get_their_roles() {
        assert_eq!(role_for(PANEL_APP_ID), Role::Panel);
        assert_eq!(role_for(LAUNCHER_APP_ID), Role::Launcher);
    }

    #[test]
    fn ordinary_applications_stay_ordinary() {
        for app_id in ["foot", "firefox", "hype-files", ""] {
            assert_eq!(role_for(app_id), Role::Normal, "{app_id}");
        }
    }

    #[test]
    fn a_lookalike_identifier_does_not_get_special_treatment() {
        // Чужое приложение не должно притвориться панелью, добавив суффикс.
        assert_eq!(role_for("dev.hypede.Shell.Evil"), Role::Normal);
        assert_eq!(role_for("dev.hypede.shell"), Role::Normal);
    }
}
