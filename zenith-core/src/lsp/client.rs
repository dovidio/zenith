//! One language server: a child process, two pipes, and the part of JSON-RPC that
//! navigation needs.
//!
//! Three decisions live here.
//!
//! 1. **Nothing waits.** Requests are written and forgotten; answers arrive on a channel
//!    that the window drains once a frame. There is no blocking call anywhere a reader
//!    can feel. The one exception is [`Client::wait`], which exists for the command
//!    line, where waiting *is* the interface.
//! 2. **Zenith answers every question with "nothing".** Servers ask clients to register
//!    capabilities, supply configuration, apply edits. A reader grants none of it, so
//!    the answer is uniformly `null` — the reply that means "not done", never "done".
//! 3. **The server never reads a stale document.** We do not edit, so the file on disk
//!    is the document: its bytes are sent at the moment of the question and no copy is
//!    kept.

use std::collections::VecDeque;
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{text, wire};

pub enum Incoming {
    /// The server answered one of our requests.
    Reply {
        id: i64,
        answer: Result<Value, String>,
    },
    /// The server asked us something.
    Asked { id: i64 },
    /// The stream ended: the server exited, or the pipe broke.
    Gone,
}

/// What the server says about its own work.
///
/// A freshly started server is busy: it is reading the project, and a question asked
/// during that reading gets a technically-correct "nothing". The protocol has two ways of
/// saying so — `$/progress` (begin/end) and rust-analyzer's `experimental/serverStatus`
/// (a `quiescent` flag) — so both are watched, and a server that never mentions it is
/// given a moment of silence before being taken at its word.
#[derive(Default)]
struct Load {
    busy: AtomicBool,
    ever: AtomicBool,
}

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    inbox: Receiver<Incoming>,
    next_id: i64,
    /// The last few lines the server printed to stderr. Only ever read to explain a
    /// server that has died or refused, so it is a short tail rather than a log.
    said: Arc<Mutex<VecDeque<String>>>,
    load: Arc<Load>,
    started: Instant,
}

impl Client {
    /// Start a server in `root`, and take its stdin and stdout.
    pub fn start(binary: &Path, args: &[String], root: &Path) -> Result<Client, String> {
        let mut child = Command::new(binary)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", binary.display()))?;
        let stdin = child.stdin.take().ok_or("the server has no stdin")?;
        let stdout = child.stdout.take().ok_or("the server has no stdout")?;
        let stderr = child.stderr.take();

        let (tx, inbox) = channel();
        let feeder = tx.clone();
        let load = Arc::new(Load::default());
        let heard = Arc::clone(&load);
        std::thread::spawn(move || {
            let mut input = BufReader::new(stdout);
            while let Ok(Some(body)) = wire::read(&mut input) {
                let Ok(message) = serde_json::from_slice::<Value>(&body) else {
                    continue;
                };
                let id = message.get("id").and_then(Value::as_i64);
                let method = message.get("method").and_then(Value::as_str).unwrap_or("");
                match (message.get("method").is_some(), id) {
                    // A request from the server: we will answer it "nothing".
                    (true, Some(id)) => {
                        if feeder.send(Incoming::Asked { id }).is_err() {
                            return;
                        }
                    }
                    // A notification. Navigation does not follow a server's narration,
                    // with one exception: how busy it is, because that is the difference
                    // between "nothing is defined there" and "I have not read the project
                    // yet".
                    (true, None) => match method {
                        "$/progress" => {
                            match message["params"]["value"]["kind"].as_str().unwrap_or("") {
                                "begin" => {
                                    heard.ever.store(true, Ordering::Relaxed);
                                    heard.busy.store(true, Ordering::Relaxed);
                                }
                                "end" => heard.busy.store(false, Ordering::Relaxed),
                                _ => {}
                            }
                        }
                        "experimental/serverStatus" => {
                            if let Some(quiet) = message["params"]["quiescent"].as_bool() {
                                heard.ever.store(true, Ordering::Relaxed);
                                heard.busy.store(!quiet, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    },
                    (false, Some(id)) => {
                        let answer = match message.get("error") {
                            Some(error) => Err(error
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("the server refused")
                                .to_string()),
                            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        if feeder.send(Incoming::Reply { id, answer }).is_err() {
                            return;
                        }
                    }
                    (false, None) => {}
                }
            }
            let _ = feeder.send(Incoming::Gone);
        });

        let said = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = stderr {
            let said = Arc::clone(&said);
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stderr)
                    .lines()
                    .map_while(Result::ok)
                {
                    // Kept as it arrives, not at exit: a server that has gone quiet
                    // usually explained itself *before* going quiet, and the explanation
                    // is the whole message.
                    if let Ok(mut tail) = said.lock() {
                        tail.push_back(line);
                        while tail.len() > 20 {
                            tail.pop_front();
                        }
                    }
                }
            });
        }

        load.busy.store(true, Ordering::Relaxed);
        Ok(Client {
            child,
            stdin,
            inbox,
            next_id: 0,
            said,
            load,
            started: Instant::now(),
        })
    }

    /// The handshake. Everything after it depends on what the server admits to.
    pub fn initialize(&mut self, root: &Path) -> Result<i64, String> {
        let uri = text::to_uri(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".into());
        self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "zenith", "version": env!("CARGO_PKG_VERSION") },
                "rootUri": uri,
                "workspaceFolders": [{ "uri": uri, "name": name }],
                "capabilities": {
                    // utf-16 is what zenith's own arithmetic assumes (see `text`).
                    // Saying so stops a server from choosing utf-8.
                    "general": { "positionEncodings": ["utf-16"] },
                    "textDocument": {
                        // Ask for LocationLink: it carries the range of the *name*,
                        // which is where a reader wants to land, not the whole body.
                        "definition": { "linkSupport": true },
                        "references": {},
                        // Without this, a server is required to answer the outline
                        // *flat*: every method of every impl as a sibling of its type.
                        // With it, the outline has the nesting the file has.
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                        "synchronization": { "dynamicRegistration": false, "didSave": false }
                    },
                    "workspace": { "workspaceFolders": true, "configuration": false }
                }
            }),
        )
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<i64, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn send(&mut self, message: Value) -> Result<(), String> {
        let body = message.to_string();
        wire::write(&mut self.stdin, &body).map_err(|e| format!("the server stopped reading: {e}"))
    }

    /// Answer a server's request. Always with nothing: no capabilities, no configuration,
    /// no applied edit, no granted permission.
    pub fn answer(&mut self, id: i64) -> Result<(), String> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }))
    }

    /// Has the server finished reading the project?
    ///
    /// A server that has never said anything about its own progress is not hiding
    /// anything — it simply has none to report — so silence is taken for readiness after
    /// a moment, rather than leaving the question hanging forever.
    pub fn is_idle(&self) -> bool {
        if !self.load.busy.load(Ordering::Relaxed) {
            return true;
        }
        !self.load.ever.load(Ordering::Relaxed)
            && self.started.elapsed() > Duration::from_millis(2_500)
    }

    pub fn poll(&self) -> Vec<Incoming> {
        self.inbox.try_iter().collect()
    }

    /// The tail of what the server printed to stderr, most recent last.
    pub fn said(&self) -> Vec<String> {
        self.said
            .lock()
            .map(|t| t.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The last thing the server said, which is usually the whole explanation.
    pub fn last_word(&self) -> String {
        self.said()
            .into_iter()
            .map(|l| l.trim().to_string())
            .rfind(|l| !l.is_empty())
            .unwrap_or_default()
    }

    /// Block until the answer to `id` arrives. For the command line only.
    pub fn wait(&mut self, id: i64, within: Duration) -> Result<Value, String> {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("the server did not answer".into());
            }
            match self.inbox.recv_timeout(left) {
                Ok(Incoming::Reply { id: got, answer }) if got == id => return answer,
                Ok(Incoming::Asked { id }) => {
                    let _ = self.answer(id);
                }
                Ok(Incoming::Gone) | Err(_) => {
                    return Err(match self.last_word() {
                        w if w.is_empty() => "the server went away".to_string(),
                        w => format!("the server went away: {w}"),
                    })
                }
                Ok(_) => {}
            }
        }
    }

    /// Ask the server to stop, and make sure it has.
    ///
    /// A language server left behind is not harmless: rust-analyzer on a large crate is
    /// a gigabyte of memory, and it survives its reader.
    pub fn stop(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Even on a panic path, a server must not outlive the window.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
