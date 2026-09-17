//! Протокол управления композитором HypeDE.
//!
//! По сокету в `$XDG_RUNTIME_DIR` можно спросить состояние среды, выполнить
//! любое действие и подписаться на события. На этом протоколе держатся панель,
//! настройки и утилита `hypectl`; на нём же пишутся пользовательские скрипты.
//!
//! # Пример
//!
//! ```no_run
//! use hype_ipc::{Client, Request, Response};
//!
//! let mut client = Client::connect_default()?;
//! if let Response::Windows { windows } = client.request(Request::ListWindows)? {
//!     for window in windows {
//!         println!("{} — {}", window.app_id, window.title);
//!     }
//! }
//! # Ok::<(), hype_ipc::IpcError>(())
//! ```

#![deny(rust_2018_idioms)]

mod protocol;
mod transport;

pub use protocol::{
    Event, EventKind, Outgoing, OutputInfo, Request, Response, State, WindowInfo, WorkspaceInfo,
};
pub use transport::{
    read_message, write_message, Client, Connection, EventStream, IpcError, Listener,
};

/// Версия протокола. Растёт при несовместимых изменениях сообщений.
pub const PROTOCOL_VERSION: u32 = 1;
