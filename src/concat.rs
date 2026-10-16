//! Client for Concat's JSON-RPC lines API (Settings > Remote in the app).
//! One connection, reconnected forever; calls are matched to replies by id.
//! Wire format: concat-api/src/rpc.rs and concat-server/src/json.rs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

const DEFAULT_LISTEN: &str = "127.0.0.1:7420";
const RETRY: Duration = Duration::from_secs(3);
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Offline,
    /// The app is up but refused the token, or has none set.
    Unauthorized,
    Online {
        version: String,
    },
}

/// A JSON-RPC notification: `export.progress`, `export.done`, ...
#[derive(Clone, Debug)]
pub struct Event {
    pub method: String,
    pub params: Value,
}

#[derive(Debug)]
pub struct CallError {
    /// Concat's error code ("notOpen", "refused", "busy", ...), or
    /// "offline" / "timeout" from this side.
    pub code: String,
    pub message: String,
}

type Pending = Mutex<HashMap<u64, oneshot::Sender<Result<Value, CallError>>>>;

pub struct Client {
    next_id: AtomicU64,
    pending: Pending,
    outbox: Mutex<Option<mpsc::UnboundedSender<String>>>,
    status: watch::Sender<Status>,
    events: broadcast::Sender<Event>,
}

static CLIENT: OnceLock<&'static Client> = OnceLock::new();

pub fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        let client: &'static Client = Box::leak(Box::new(Client {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            outbox: Mutex::new(None),
            status: watch::channel(Status::Offline).0,
            events: broadcast::channel(256).0,
        }));
        tokio::spawn(client.run());
        client
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AppSettings {
    server: ServerPrefs,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ServerPrefs {
    listen: String,
    token: String,
}

/// The app's own settings file: concat-host/src/dirs.rs, concat/src/prefs.rs.
fn app_settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/app.concat.editor/settings.json"))
}

fn read_endpoint() -> (String, String) {
    let prefs = app_settings_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<AppSettings>(&bytes).ok())
        .unwrap_or_default()
        .server;
    let listen = if prefs.listen.trim().is_empty() {
        DEFAULT_LISTEN.to_owned()
    } else {
        prefs.listen.trim().to_owned()
    };
    (listen, prefs.token)
}

impl Client {
    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        {
            let outbox = self.outbox.lock().unwrap();
            let Some(outbox) = outbox.as_ref() else {
                return Err(offline());
            };
            self.pending.lock().unwrap().insert(id, tx);
            if outbox.send(line.to_string()).is_err() {
                self.pending.lock().unwrap().remove(&id);
                return Err(offline());
            }
        }
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(offline()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(CallError {
                    code: "timeout".into(),
                    message: format!("{method} timed out"),
                })
            }
        }
    }

    async fn run(&'static self) {
        loop {
            let (listen, token) = read_endpoint();
            match self.session(&listen, &token).await {
                Ok(()) => log::info!("concat: disconnected"),
                Err(why) => log::debug!("concat: {why}"),
            }
            *self.outbox.lock().unwrap() = None;
            for (_, waiter) in self.pending.lock().unwrap().drain() {
                let _ = waiter.send(Err(offline()));
            }
            tokio::time::sleep(RETRY).await;
        }
    }

    async fn session(&'static self, listen: &str, token: &str) -> Result<(), String> {
        let stream = match TcpStream::connect(listen).await {
            Ok(stream) => stream,
            Err(error) => {
                self.status
                    .send_if_modified(|s| replace(s, Status::Offline));
                return Err(format!("connect {listen}: {error}"));
            }
        };
        stream.set_nodelay(true).ok();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();

        let auth =
            json!({ "jsonrpc": "2.0", "id": 0, "method": "auth", "params": { "token": token } });
        write
            .write_all(format!("{auth}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let reply = lines
            .next_line()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("closed during auth")?;
        let reply: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
        if reply.get("error").is_some() {
            self.status
                .send_if_modified(|s| replace(s, Status::Unauthorized));
            return Err(format!("auth refused: {}", reply["error"]["message"]));
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        *self.outbox.lock().unwrap() = Some(tx);
        let writer = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if write
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let reader = async {
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                self.dispatch(message);
            }
        };
        let version = async {
            let info = self.call("version", json!({})).await;
            let version = info
                .ok()
                .and_then(|v| v.get("concat").cloned())
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            log::info!("concat: connected to {listen} ({version})");
            self.status
                .send_if_modified(|s| replace(s, Status::Online { version }));
            std::future::pending::<()>().await
        };
        tokio::select! {
            () = reader => {}
            () = version => {}
        }
        writer.abort();
        self.status
            .send_if_modified(|s| replace(s, Status::Offline));
        Ok(())
    }

    fn dispatch(&self, message: Value) {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let _ = self.events.send(Event {
                method: method.to_owned(),
                params: message.get("params").cloned().unwrap_or(Value::Null),
            });
            return;
        }
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        let Some(waiter) = self.pending.lock().unwrap().remove(&id) else {
            return;
        };
        let result = match message.get("error") {
            Some(error) => Err(CallError {
                code: error["data"]["code"]
                    .as_str()
                    .unwrap_or("failed")
                    .to_owned(),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
            }),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = waiter.send(result);
    }
}

fn offline() -> CallError {
    CallError {
        code: "offline".into(),
        message: "Concat is not reachable".into(),
    }
}

fn replace(slot: &mut Status, next: Status) -> bool {
    if *slot == next {
        return false;
    }
    *slot = next;
    true
}
