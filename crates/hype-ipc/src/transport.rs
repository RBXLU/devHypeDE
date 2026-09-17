//! Транспорт: кадрирование сообщений и работа с сокетом.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::protocol::{Event, EventKind, Outgoing, Request, Response};

/// Ошибки канала управления.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),
    #[error("сообщение не разобрано: {0}")]
    Json(#[from] serde_json::Error),
    #[error("соединение закрыто собеседником")]
    Disconnected,
    #[error("не удалось определить путь к сокету: не задан XDG_RUNTIME_DIR")]
    NoSocketPath,
    #[error("композитор HypeDE не отвечает по адресу {0}")]
    NotRunning(PathBuf),
}

/// Записывает одно сообщение и сразу проталкивает буфер.
///
/// Без явного сброса ответ может застрять в буфере, и клиент будет ждать
/// сообщение, которое уже сформировано, но ещё не ушло.
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), IpcError> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()?;
    Ok(())
}

/// Читает одно сообщение. `Ok(None)` означает, что поток закончился.
pub fn read_message<R: BufRead, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, IpcError> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        // Пустые строки пропускаем: они ничего не значат и не должны ронять
        // разбор потока.
        if line.trim().is_empty() {
            continue;
        }
        return Ok(Some(serde_json::from_str(&line)?));
    }
}

/// Клиент управления композитором.
#[derive(Debug)]
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// Подключается к сокету по указанному пути.
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(path).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                IpcError::NotRunning(path.to_path_buf())
            }
            _ => IpcError::Io(err),
        })?;
        Self::from_stream(stream)
    }

    /// Подключается к сокету текущего сеанса.
    pub fn connect_default() -> Result<Self, IpcError> {
        let path = hype_config::paths::control_socket().ok_or(IpcError::NoSocketPath)?;
        Self::connect(&path)
    }

    fn from_stream(stream: UnixStream) -> Result<Self, IpcError> {
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
        })
    }

    /// Отправляет запрос и ждёт ответ.
    ///
    /// События, пришедшие в этот момент (клиент мог подписаться раньше),
    /// пропускаются: у `request` есть обязательство вернуть именно ответ.
    pub fn request(&mut self, request: Request) -> Result<Response, IpcError> {
        write_message(&mut self.writer, &request)?;
        loop {
            match read_message::<_, Outgoing>(&mut self.reader)? {
                Some(Outgoing::Response(response)) => return Ok(response),
                Some(Outgoing::Event(_)) => continue,
                None => return Err(IpcError::Disconnected),
            }
        }
    }

    /// Подписывается на события и превращает соединение в поток событий.
    pub fn subscribe(mut self, events: Vec<EventKind>) -> Result<EventStream, IpcError> {
        let response = self.request(Request::Subscribe { events })?;
        if let Response::Error { message } = response {
            return Err(IpcError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            ))));
        }
        Ok(EventStream { client: self })
    }
}

/// Бесконечный поток событий от композитора.
#[derive(Debug)]
pub struct EventStream {
    client: Client,
}

impl Iterator for EventStream {
    type Item = Result<Event, IpcError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match read_message::<_, Outgoing>(&mut self.client.reader) {
                Ok(Some(Outgoing::Event(event))) => return Some(Ok(event)),
                // Ответы в потоке событий не наше дело — идём дальше.
                Ok(Some(Outgoing::Response(_))) => continue,
                Ok(None) => return None,
                Err(err) => return Some(Err(err)),
            }
        }
    }
}

/// Слушающий сокет композитора.
#[derive(Debug)]
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Открывает сокет, убрав за предыдущим сеансом.
    ///
    /// Оставшийся от упавшего композитора файл сокета не даёт создать новый,
    /// поэтому мёртвый файл удаляется. Живой — нет: иначе второй запуск тихо
    /// отбирал бы управление у работающего сеанса.
    pub fn bind(path: &Path) -> Result<Self, IpcError> {
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(IpcError::Io(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("по адресу {} уже работает композитор", path.display()),
                )));
            }
            std::fs::remove_file(path)?;
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        Ok(Self {
            inner: UnixListener::bind(path)?,
            path: path.to_path_buf(),
        })
    }

    /// Открывает сокет текущего сеанса.
    pub fn bind_default() -> Result<Self, IpcError> {
        let path = hype_config::paths::control_socket().ok_or(IpcError::NoSocketPath)?;
        Self::bind(&path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Доступ к сокету для интеграции с циклом событий композитора.
    pub fn inner(&self) -> &UnixListener {
        &self.inner
    }

    /// Принимает одно соединение.
    pub fn accept(&self) -> Result<Connection, IpcError> {
        let (stream, _) = self.inner.accept()?;
        Connection::new(stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // Файл сокета не исчезает сам — убираем, чтобы следующий запуск не
        // упирался в чужой мусор.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Одно клиентское соединение со стороны композитора.
#[derive(Debug)]
pub struct Connection {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    /// На что подписан клиент. Пусто — значит, событий не ждёт.
    pub subscriptions: Vec<EventKind>,
}

impl Connection {
    pub fn new(stream: UnixStream) -> Result<Self, IpcError> {
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            subscriptions: Vec::new(),
        })
    }

    /// Читает очередной запрос. `None` — клиент отключился.
    pub fn read_request(&mut self) -> Result<Option<Request>, IpcError> {
        read_message(&mut self.reader)
    }

    pub fn send_response(&mut self, response: &Response) -> Result<(), IpcError> {
        write_message(&mut self.writer, &Outgoing::Response(response.clone()))
    }

    /// Отправляет событие, если клиент на него подписан.
    pub fn send_event(&mut self, event: &Event) -> Result<(), IpcError> {
        if !self.subscriptions.contains(&event.kind()) {
            return Ok(());
        }
        write_message(&mut self.writer, &Outgoing::Event(event.clone()))
    }

    /// Переводит соединение в режим подписки.
    pub fn subscribe(&mut self, events: Vec<EventKind>) {
        self.subscriptions = events;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::WindowInfo;
    use std::io::Cursor;

    fn window() -> WindowInfo {
        WindowInfo {
            id: 7,
            title: "Файлы".into(),
            app_id: "hype-files".into(),
            workspace: 1,
            x: 10,
            y: 20,
            width: 900,
            height: 700,
            focused: true,
            floating: false,
            fullscreen: false,
        }
    }

    #[test]
    fn messages_are_framed_one_per_line() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &Request::Ping).unwrap();
        write_message(&mut buffer, &Request::GetState).unwrap();

        let text = String::from_utf8(buffer).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn framed_messages_read_back_in_order() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &Request::Ping).unwrap();
        write_message(&mut buffer, &Request::ListWindows).unwrap();

        let mut reader = Cursor::new(buffer);
        assert_eq!(
            read_message::<_, Request>(&mut reader).unwrap(),
            Some(Request::Ping)
        );
        assert_eq!(
            read_message::<_, Request>(&mut reader).unwrap(),
            Some(Request::ListWindows)
        );
        assert_eq!(read_message::<_, Request>(&mut reader).unwrap(), None);
    }

    #[test]
    fn blank_lines_are_skipped() {
        let mut reader = Cursor::new(b"\n\n{\"request\":\"ping\"}\n".to_vec());
        assert_eq!(
            read_message::<_, Request>(&mut reader).unwrap(),
            Some(Request::Ping)
        );
    }

    #[test]
    fn a_malformed_line_is_an_error_with_context() {
        let mut reader = Cursor::new(b"{not json}\n".to_vec());
        let err = read_message::<_, Request>(&mut reader).unwrap_err();
        assert!(matches!(err, IpcError::Json(_)), "{err:?}");
    }

    fn socket_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hype-ipc-test-{name}-{}.sock", std::process::id()))
    }

    #[test]
    fn a_request_gets_its_response_over_a_real_socket() {
        let path = socket_path("request");
        let listener = Listener::bind(&path).unwrap();

        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().unwrap();
            let request = connection.read_request().unwrap().unwrap();
            assert_eq!(request, Request::ListWindows);
            connection
                .send_response(&Response::Windows {
                    windows: vec![window()],
                })
                .unwrap();
        });

        let mut client = Client::connect(&path).unwrap();
        let response = client.request(Request::ListWindows).unwrap();
        assert_eq!(
            response,
            Response::Windows {
                windows: vec![window()]
            }
        );

        server.join().unwrap();
    }

    #[test]
    fn subscribers_receive_only_the_events_they_asked_for() {
        let path = socket_path("subscribe");
        let listener = Listener::bind(&path).unwrap();

        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().unwrap();
            let Some(Request::Subscribe { events }) = connection.read_request().unwrap() else {
                panic!("ожидалась подписка");
            };
            connection.subscribe(events);
            connection.send_response(&Response::Ok).unwrap();

            // На это клиент не подписывался — оно не должно дойти.
            connection
                .send_event(&Event::WorkspaceChanged { index: 4 })
                .unwrap();
            connection
                .send_event(&Event::WindowClosed { id: 7 })
                .unwrap();
        });

        let client = Client::connect(&path).unwrap();
        let mut events = client.subscribe(vec![EventKind::Window]).unwrap();

        let first = events.next().unwrap().unwrap();
        assert_eq!(first, Event::WindowClosed { id: 7 });

        server.join().unwrap();
    }

    #[test]
    fn event_stream_ends_when_the_compositor_goes_away() {
        let path = socket_path("hangup");
        let listener = Listener::bind(&path).unwrap();

        let server = std::thread::spawn(move || {
            let mut connection = listener.accept().unwrap();
            let _ = connection.read_request().unwrap();
            connection.subscribe(vec![EventKind::Window]);
            connection.send_response(&Response::Ok).unwrap();
            // Соединение закрывается вместе с потоком.
        });

        let client = Client::connect(&path).unwrap();
        let mut events = client.subscribe(vec![EventKind::Window]).unwrap();
        assert!(events.next().is_none());

        server.join().unwrap();
    }

    #[test]
    fn connecting_to_nothing_reports_that_the_compositor_is_not_running() {
        let path = socket_path("missing");
        let _ = std::fs::remove_file(&path);
        let err = Client::connect(&path).unwrap_err();
        assert!(matches!(err, IpcError::NotRunning(_)), "{err:?}");
    }

    #[test]
    fn a_stale_socket_file_does_not_block_startup() {
        let path = socket_path("stale");
        std::fs::write(&path, "мусор от упавшего сеанса".as_bytes()).unwrap();
        let listener = Listener::bind(&path).unwrap();
        assert!(listener.path().exists());
    }

    #[test]
    fn a_second_compositor_refuses_to_steal_the_socket() {
        let path = socket_path("busy");
        let _first = Listener::bind(&path).unwrap();
        let err = Listener::bind(&path).unwrap_err();
        assert!(err.to_string().contains("уже работает"), "{err}");
    }

    #[test]
    fn dropping_the_listener_removes_the_socket_file() {
        let path = socket_path("cleanup");
        {
            let _listener = Listener::bind(&path).unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists(), "файл сокета остался после завершения");
    }
}
