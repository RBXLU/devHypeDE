//! Оболочка HypeDE: панель и поиск приложений.

#![deny(rust_2018_idioms)]

pub mod desktop;
pub mod launcher;
pub mod panel;
pub mod status;
pub mod theme;

/// Идентификатор панели. Композитор узнаёт по нему окно и кладёт его в
/// отведённую полосу вместо общей раскладки.
pub const PANEL_APP_ID: &str = "dev.hypede.Shell";

/// Идентификатор окна поиска приложений.
pub const LAUNCHER_APP_ID: &str = "dev.hypede.Launcher";
