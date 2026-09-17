//! Сервер протокола управления.
//!
//! Сокет обслуживается отдельными потоками, а сами запросы исполняются в
//! главном цикле: трогать состояние композитора из чужого потока нельзя.
//! Поток-обработчик передаёт запрос по каналу calloop и ждёт ответ обратно по
//! обычному каналу std.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use hype_ipc::{
    write_message, Event, EventKind, Listener, Outgoing, Request, Response,
};
use smithay::reexports::calloop::channel::Sender as LoopSender;
use tracing::{debug, warn};

use crate::state::HypeState;

/// Запрос, ждущий исполнения в главном цикле.
pub struct PendingRequest {
    pub request: Request,
    pub reply: mpsc::Sender<Response>,
}

struct Subscriber {
    kinds: Vec<EventKind>,
    stream: std::os::unix::net::UnixStream,
}

/// Сервер управления, живущий рядом с композитором.
pub struct IpcServer {
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
    path: PathBuf,
}

impl IpcServer {
    /// Запускает приём соединений.
    ///
    /// Сокет уже открыт вызывающим кодом: так ошибка «композитор уже запущен»
    /// всплывает до того, как сеанс начнёт что-то делать.
    pub fn start(listener: Listener, requests: LoopSender<PendingRequest>) -> Self {
        let subscribers: Arc<Mutex<Vec<Subscriber>>> = Arc::new(Mutex::new(Vec::new()));
        let path = listener.path().to_path_buf();

        let accept_subscribers = Arc::clone(&subscribers);
        std::thread::Builder::new()
            .name("hype-ipc-accept".into())
            .spawn(move || {
                loop {
                    let connection = match listener.accept() {
                        Ok(connection) => connection,
                        Err(err) => {
                            warn!("не удалось принять соединение управления: {err}");
                            continue;
                        }
                    };

                    let subscribers = Arc::clone(&accept_subscribers);
                    let requests = requests.clone();
                    std::thread::Builder::new()
                        .name("hype-ipc-client".into())
                        .spawn(move || serve_client(connection, subscribers, requests))
                        .ok();
                }
            })
            .expect("не удалось запустить поток управления");

        Self { subscribers, path }
    }

    /// Рассылает событие подписчикам.
    ///
    /// Отвалившиеся клиенты молча выбрасываются: закрытая панель — обычное
    /// дело, а не повод писать в журнал на каждое движение окна.
    pub fn broadcast(&self, event: &Event) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        let kind = event.kind();
        subscribers.retain_mut(|subscriber| {
            if !subscriber.kinds.contains(&kind) {
                return true;
            }
            write_message(&mut subscriber.stream, &Outgoing::Event(event.clone())).is_ok()
        });
    }

    /// Сколько клиентов сейчас слушает события.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.lock().map(|s| s.len()).unwrap_or(0)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // Поток приёма живёт до конца процесса, поэтому файл сокета убираем
        // здесь — иначе следующий запуск наткнётся на чужой мусор.
        let _ = std::fs::remove_file(&self.path);
    }
}

fn serve_client(
    mut connection: hype_ipc::Connection,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
    requests: LoopSender<PendingRequest>,
) {
    loop {
        let request = match connection.read_request() {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(err) => {
                debug!("соединение управления закрыто: {err}");
                break;
            }
        };

        if let Request::Subscribe { events } = request {
            match connection.take_stream() {
                Some(stream) => {
                    let mut subscriber = Subscriber {
                        kinds: events,
                        stream,
                    };
                    let ok = write_message(&mut subscriber.stream, &Outgoing::Response(Response::Ok))
                        .is_ok();
                    if ok {
                        if let Ok(mut list) = subscribers.lock() {
                            list.push(subscriber);
                        }
                    }
                }
                None => {
                    let _ = connection.send_response(&Response::error(
                        "не удалось перевести соединение в режим подписки",
                    ));
                }
            }
            // Дальше это соединение живёт только на отправку событий.
            break;
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        if requests
            .send(PendingRequest {
                request,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }

        match reply_rx.recv() {
            Ok(response) => {
                if connection.send_response(&response).is_err() {
                    break;
                }
            }
            // Главный цикл завершился — отвечать больше некому.
            Err(_) => break,
        }
    }
}

impl HypeState {
    /// Исполняет запрос управления.
    pub fn handle_ipc_request(&mut self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong,
            Request::Version => Response::Version {
                version: crate::VERSION.to_string(),
            },
            Request::GetState => Response::State(Box::new(self.ipc_state())),
            Request::ListWindows => Response::Windows {
                windows: self
                    .windows
                    .keys()
                    .filter_map(|id| self.window_info(*id))
                    .collect(),
            },
            Request::ListWorkspaces => Response::Workspaces {
                workspaces: self.workspace_infos(),
            },
            Request::GetConfig => Response::Config {
                config: Box::new(self.config.clone()),
            },
            Request::Dispatch { action } => {
                self.dispatch(&action);
                Response::Ok
            }
            Request::ApplyConfig { config } => {
                self.apply_config(*config);
                Response::Ok
            }
            // Подписка обрабатывается в потоке соединения и сюда не доходит.
            Request::Subscribe { .. } => {
                Response::error("подписка обрабатывается вне главного цикла")
            }
        }
    }
}
