//! Language servers, for navigation and nothing else.
//!
//! Zenith asks a server two questions — *where is this defined*, and *where is this
//! used* — and never anything else. No completion, no diagnostics, no inlay hints, no
//! rename: a reader does not change code, so the client never sends `didChange`, never
//! sends `applyEdit`, and answers every question a server asks with "nothing".
//!
//! Stating what is *not* here is the point, because "LSP support" usually means the
//! opposite: a workspace that must be indexed before the page is usable, a strip of
//! squiggles competing with the prose, a gigabyte of rust-analyzer that outlives the
//! window. Navigation is different in kind — it is a question asked by a key press and
//! answered in milliseconds, and when there is no server to ask, the reader keeps
//! reading and the status strip says why.
//!
//! Four pieces:
//!
//! * [`servers`] — the table of which server serves which file, and where it may be.
//! * [`client`] — the child process, the frames, the handshake.
//! * [`text`] — the arithmetic between a caret, a UTF-16 position, and a file's bytes.
//! * this module — a [`Session`]: a server that has been started, the state it is in, and
//!   the one question it can answer today.

pub mod client;
pub mod servers;
pub mod text;
mod wire;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use client::{Client, Incoming};

/// A place a server says something is, in the file's own coordinates.
///
/// The column is *UTF-16 code units from the start of the line*, as the protocol counts
/// them; the caller turns it into a caret column against the line it just opened, which
/// is the only moment the two can be compared. See [`text::column`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    /// Zero-based, like the protocol.
    pub line: usize,
    pub unit: u32,
}

impl Target {
    /// Where to say it is, for a person: `~/code/x/src/lib.rs:118`.
    pub fn place(&self) -> String {
        format!(
            "{}:{}",
            crate::catalog::display_path(&self.path),
            self.line + 1
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// The handshake is in flight; nothing can be asked yet.
    Starting,
    Ready,
    /// The server refused the handshake or exited. The string is what it said.
    Failed(String),
}

/// One entry of a document's outline, as the server sees it.
#[derive(Clone, Debug)]
pub struct Outline {
    pub name: String,
    pub line: usize,
    pub kind: crate::outline::SymKind,
    pub depth: usize,
    /// What follows the name on its line, for context: `fn memo(&self) -> String`.
    pub detail: String,
}

/// What a session has to report after a frame's worth of messages.
pub enum Answer {
    Ready,
    /// The session is over, and this is why.
    Ended(String),
    /// Targets for the request with this id.
    Found {
        id: i64,
        targets: Vec<Target>,
    },
    /// The server answered a question with an error.
    Refused {
        id: i64,
        message: String,
    },
    /// The outline of the document that was open when it was asked for.
    Outline(Vec<Outline>),
}

pub struct Session {
    pub kind: &'static servers::Kind,
    pub root: PathBuf,
    client: Client,
    state: State,
    handshake: i64,
    /// The document the server has been told about, if any.
    doc: Option<PathBuf>,
    /// The document symbol request in flight, if any.
    symbols: Option<i64>,
}

impl Session {
    pub fn start(
        kind: &'static servers::Kind,
        found: &servers::Available,
        root: &Path,
    ) -> Result<Session, String> {
        let mut client = Client::start(&found.binary, &found.args, root)?;
        let handshake = client.initialize(root)?;
        Ok(Session {
            kind,
            root: root.to_path_buf(),
            client,
            state: State::Starting,
            handshake,
            doc: None,
            symbols: None,
        })
    }

    pub fn id(&self) -> &'static str {
        self.kind.id
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Take one frame's worth of messages. Never blocks.
    pub fn poll(&mut self) -> Vec<Answer> {
        let mut answers = Vec::new();
        for message in self.client.poll() {
            match message {
                Incoming::Asked { id } => {
                    let _ = self.client.answer(id);
                }
                Incoming::Gone => answers.push(self.ended()),
                Incoming::Reply { id, answer }
                    if id == self.handshake && self.state == State::Starting =>
                {
                    match answer {
                        Ok(_) => {
                            let _ = self.client.notify("initialized", json!({}));
                            self.state = State::Ready;
                            answers.push(Answer::Ready);
                        }
                        Err(e) => {
                            self.state = State::Failed(e.clone());
                            answers.push(Answer::Ended(e));
                        }
                    }
                }
                Incoming::Reply { id, answer } if Some(id) == self.symbols => {
                    self.symbols = None;
                    // A refused outline is not worth a message: the panel keeps what the
                    // grammar guessed, which is what it had before the server answered.
                    if let Ok(value) = answer {
                        answers.push(Answer::Outline(outline_of(&value)));
                    }
                }
                Incoming::Reply { id, answer } => match answer {
                    Ok(value) => answers.push(Answer::Found {
                        id,
                        targets: targets(&value),
                    }),
                    Err(message) => answers.push(Answer::Refused { id, message }),
                },
            }
        }
        answers
    }

    /// Where is the thing at this place defined?
    ///
    /// Takes the position in protocol coordinates; see [`text::position`].
    pub fn definition(&mut self, file: &Path, line: u32, unit: u32) -> Result<i64, String> {
        self.open(file)?;
        self.client.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": text::to_uri(file) },
                "position": { "line": line, "character": unit }
            }),
        )
    }

    /// Ask for the document's outline.
    ///
    /// A grammar can say where a declaration *starts*; only the compiler's own view of
    /// the file knows what is nested in what, and the heuristic in `outline` is the
    /// fallback for the first frame, when no server has answered yet.
    pub fn outline(&mut self, file: &Path) -> Result<i64, String> {
        self.open(file)?;
        let id = self.client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": text::to_uri(file) } }),
        )?;
        self.symbols = Some(id);
        Ok(id)
    }

    /// Tell the server about the file being read.
    ///
    /// One file at a time — the reader reads one page — and no `didChange` afterwards,
    /// because there is nothing to change. The previous document is closed so a long
    /// reading session does not leave the whole library open in the server's mind.
    fn open(&mut self, file: &Path) -> Result<(), String> {
        if self.doc.as_deref() == Some(file) {
            return Ok(());
        }
        if let Some(previous) = self.doc.take() {
            let _ = self.client.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": text::to_uri(&previous) } }),
            );
        }
        let source = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let language = servers::kind_for(file)
            .map(|(_, id)| id)
            .unwrap_or(self.kind.id);
        self.client.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": text::to_uri(file),
                "languageId": language,
                "version": 1,
                "text": source
            }}),
        )?;
        self.doc = Some(file.to_path_buf());
        Ok(())
    }

    /// Has the server finished reading the project?
    pub fn is_idle(&self) -> bool {
        self.client.is_idle()
    }

    /// Wait until it has. For the command line, and for the one retry the window makes.
    pub fn wait_idle(&mut self, within: Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + within;
        while !self.client.is_idle() {
            if std::time::Instant::now() >= deadline {
                return Err("the server is still reading this project".into());
            }
            if let Some(Answer::Ended(why)) = self
                .poll()
                .into_iter()
                .find(|a| matches!(a, Answer::Ended(_)))
            {
                return Err(why);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    fn ended(&mut self) -> Answer {
        let said = self.client.last_word();
        let message = if said.is_empty() {
            "the server exited".to_string()
        } else {
            said
        };
        self.state = State::Failed(message.clone());
        Answer::Ended(message)
    }

    /// Wait for the handshake. For the command line, where waiting is the interface.
    pub fn wait_ready(&mut self, within: Duration) -> Result<(), String> {
        let answer = self.client.wait(self.handshake, within)?;
        let _ = answer;
        let _ = self.client.notify("initialized", json!({}));
        self.state = State::Ready;
        Ok(())
    }

    /// Wait for the answer to a request. Command line only.
    pub fn wait(&mut self, id: i64, within: Duration) -> Result<Value, String> {
        self.client.wait(id, within)
    }

    pub fn stop(&mut self) {
        self.client.stop();
    }
}

/// Is this error the server saying "ask again" rather than "no"?
///
/// `ContentModified` is what a server answers when the snapshot it was asked about moved
/// while the question was in flight — routine when a document has only just been opened,
/// and the protocol's own answer is to retry. Reporting it to the reader as a failure
/// would be reporting a race as a fact about their code.
pub fn transient(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("content modified") || m.contains("content-modified")
}

/// One question, asked and waited for. This is what `--lsp-ask` runs, and it is the only
/// part of this module that blocks.
pub fn ask_definition(
    file: &Path,
    line: usize,
    unit: u32,
    root: &Path,
    over: Option<&servers::Override>,
) -> Result<Vec<Target>, String> {
    let (kind, _) = servers::kind_for(file)
        .ok_or_else(|| format!("no language server serves {}", file.display()))?;
    let found = servers::find(kind, root, over)
        .ok_or_else(|| format!("no {} language server found on this machine", kind.title))?;
    let mut session = Session::start(kind, &found, root)?;
    session.wait_ready(Duration::from_secs(30))?;
    // The handshake is not readiness: a server that has not read the project yet answers
    // "nothing", which is true and useless. Wait for it to say it is done.
    let _ = session.wait_idle(Duration::from_secs(30));
    // A freshly opened document can race the question; the server says so, and the
    // answer is to ask again rather than to report a race as a fact.
    let mut last = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(300));
        }
        let id = session.definition(file, line as u32, unit)?;
        match session.wait(id, Duration::from_secs(30)) {
            Ok(value) => {
                session.stop();
                return Ok(targets(&value));
            }
            Err(e) if transient(&e) => last = e,
            Err(e) => {
                session.stop();
                return Err(e);
            }
        }
    }
    session.stop();
    Err(last)
}

/// Read the shapes a definition or reference answer can take.
///
/// The protocol allows four: a `Location`, a list of them, a `LocationLink`, a list of
/// those — plus `null`, which is the ordinary answer when there is nothing there. A
/// link is the better of the two, because `targetSelectionRange` is the range of the
/// *name* while `targetRange` is the whole body it belongs to, and a reader wants the
/// name.
fn targets(value: &Value) -> Vec<Target> {
    let mut out = Vec::new();
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                collect(item, &mut out);
            }
        }
        other => collect(other, &mut out),
    }
    // One place can arrive twice under two names for one file: on macOS `/tmp` is a
    // symlink to `/private/tmp`, and a server that resolves the symlink answers with a
    // URI the rest of the program has never seen. Canonicalise, then drop the repeats —
    // otherwise the reader is offered the same target twice, and the tree cannot find
    // either of them.
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert((t.path.clone(), t.line, t.unit)));
    out
}

/// Read an outline out of either shape the protocol allows: a tree of `DocumentSymbol`
/// (which is what a server that knows the language sends) or a flat list of
/// `SymbolInformation` (which is what the older servers send).
fn outline_of(value: &Value) -> Vec<Outline> {
    let mut out = Vec::new();
    let Some(items) = value.as_array() else {
        return out;
    };
    let flat = items
        .iter()
        .all(|item| item.get("location").is_some() && item.get("range").is_none());
    for item in items {
        if flat {
            let Some(line) = item["location"]["range"]["start"]["line"].as_u64() else {
                continue;
            };
            out.push(Outline {
                name: item["name"].as_str().unwrap_or("").to_string(),
                line: line as usize,
                kind: kind_of(item["kind"].as_u64().unwrap_or(0)),
                depth: 0,
                detail: String::new(),
            });
        } else {
            walk_symbol(item, 0, &mut out);
        }
    }
    out.sort_by_key(|s| s.line);
    out
}

fn walk_symbol(item: &Value, depth: usize, out: &mut Vec<Outline>) {
    let Some(line) = item["selectionRange"]["start"]["line"]
        .as_u64()
        .or_else(|| item["range"]["start"]["line"].as_u64())
    else {
        return;
    };
    let name = item["name"].as_str().unwrap_or("");
    if !name.is_empty() {
        out.push(Outline {
            name: name.to_string(),
            line: line as usize,
            kind: kind_of(item["kind"].as_u64().unwrap_or(0)),
            depth,
            detail: item["detail"].as_str().unwrap_or("").to_string(),
        });
    }
    if let Some(children) = item["children"].as_array() {
        for child in children {
            walk_symbol(child, depth + 1, out);
        }
    }
}

/// The protocol's symbol kinds, folded onto the eight the page has marks for.
fn kind_of(kind: u64) -> crate::outline::SymKind {
    use crate::outline::SymKind;
    match kind {
        // file, module, namespace, package
        1..=4 => SymKind::Module,
        // class, interface, struct, type parameter
        5 | 11 | 23 | 26 => SymKind::Type,
        10 => SymKind::Enum,
        // method, constructor, function, operator
        6 | 9 | 12 | 25 => SymKind::Function,
        // property, field, variable, constant, and every literal kind
        7 | 8 | 13..=21 => SymKind::Constant,
        22 => SymKind::Constant, // enum member
        _ => SymKind::Function,
    }
}

fn collect(value: &Value, out: &mut Vec<Target>) {
    let (uri, range) = match (
        value.get("targetUri"),
        value
            .get("targetSelectionRange")
            .or_else(|| value.get("targetRange")),
    ) {
        (Some(uri), Some(range)) => (uri, range),
        _ => match (value.get("uri"), value.get("range")) {
            (Some(uri), Some(range)) => (uri, range),
            _ => return,
        },
    };
    let Some(uri) = uri.as_str() else { return };
    let Some(path) = text::from_uri(uri) else {
        return;
    };
    let Some(start) = range.get("start") else {
        return;
    };
    out.push(Target {
        path: path.canonicalize().unwrap_or(path),
        line: start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize,
        unit: start.get("character").and_then(Value::as_u64).unwrap_or(0) as u32,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_location_is_one_target() {
        let value = json!([{
            "uri": "file:///tmp/x.c",
            "range": { "start": { "line": 3, "character": 7 }, "end": { "line": 3, "character": 9 } }
        }]);
        let found = targets(&value);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].unit, 7);
    }

    #[test]
    fn a_location_link_lands_on_the_name_rather_than_the_body() {
        let value = json!({
            "targetUri": "file:///tmp/x.c",
            "targetRange": { "start": { "line": 10, "character": 0 }, "end": { "line": 20, "character": 0 } },
            "targetSelectionRange": { "start": { "line": 12, "character": 5 }, "end": { "line": 12, "character": 9 } }
        });
        let found = targets(&value);
        assert_eq!((found[0].line, found[0].unit), (12, 5));
    }

    #[test]
    fn nothing_there_is_an_empty_answer_not_an_error() {
        assert!(targets(&Value::Null).is_empty());
        assert!(targets(&json!([])).is_empty());
        // An unreadable URI — a class in a server's own archive — is not a place on
        // this disk, so it is not a target.
        assert!(targets(&json!([{ "uri": "jdt://contents/Foo.class",
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } }]))
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_same_place_under_two_names_is_offered_once() {
        // On macOS `/tmp` is a symlink to `/private/tmp`, and which name a server uses
        // is up to the server. Two names for one place would offer the reader the same
        // target twice and leave the tree unable to highlight either.
        let dir = std::env::temp_dir().join(format!("zenith-lsp-{}", std::process::id()));
        let real = dir.join("real");
        let link = dir.join("link");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file = real.join("x.c");
        std::fs::write(&file, "int x;").unwrap();

        let range =
            json!({ "start": { "line": 3, "character": 7 }, "end": { "line": 3, "character": 9 } });
        let value = json!([
            { "uri": text::to_uri(&file), "range": range },
            { "uri": text::to_uri(&link.join("x.c")), "range": range }
        ]);
        let found = targets(&value);
        assert_eq!(found.len(), 1, "two names, one place");
        assert_eq!(found[0].path, file.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
