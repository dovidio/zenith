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

use serde_json::{Value, json};

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
        format!("{}:{}", crate::catalog::display_path(&self.path), self.line + 1)
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

/// What a session has to report after a frame's worth of messages.
pub enum Answer {
    Ready,
    /// The session is over, and this is why.
    Ended(String),
    /// Targets for the request with this id.
    Found { id: i64, targets: Vec<Target> },
    /// The server answered a question with an error.
    Refused { id: i64, message: String },
}

pub struct Session {
    pub kind: &'static servers::Kind,
    pub root: PathBuf,
    client: Client,
    state: State,
    handshake: i64,
    /// The document the server has been told about, if any.
    doc: Option<PathBuf>,
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
                Incoming::Reply { id, answer } if id == self.handshake && self.state == State::Starting => {
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
            if let Some(Answer::Ended(why)) = self.poll().into_iter().find(|a| matches!(a, Answer::Ended(_))) {
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
    let id = session.definition(file, line as u32, unit)?;
    let value = session.wait(id, Duration::from_secs(30))?;
    session.stop();
    Ok(targets(&value))
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

fn collect(value: &Value, out: &mut Vec<Target>) {
    let (uri, range) = match (
        value.get("targetUri"),
        value.get("targetSelectionRange").or_else(|| value.get("targetRange")),
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

        let range = json!({ "start": { "line": 3, "character": 7 }, "end": { "line": 3, "character": 9 } });
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
