//! Композитор HypeDE.
//!
//! Крейт разделён надвое:
//!
//! * чистая логика — раскладка, рабочие столы, анимации окон. Ни одной строки
//!   Wayland, всё проверяется обычными тестами;
//! * обвязка smithay — состояние сеанса, обработчики протоколов, ввод и вывод.
//!
//! Разделение не косметическое: поведение среды (куда уйдёт фокус, где окажется
//! окно) отлаживается тестами за секунды, а не запуском сеанса и кликаньем.

#![deny(rust_2018_idioms)]

mod actions;
pub mod cursor;
pub mod drm;
mod handlers;
mod input;
pub mod ipc;
pub mod layout;
pub mod render;
pub mod roles;
pub mod state;
pub mod wallpaper;
pub mod window_anim;
pub mod winit;
pub mod workspace;

/// Версия композитора.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
