//! Language servers: discovery, definitions, outlines and their quiet failures.
//!
//! Nothing here writes to the project. A server is asked a question, answers are read
//! on later frames, and a missing server is a sentence in the strip rather than an
//! error. The status line itself lives on the window, not here.

use super::*;

impl Zenith {
    /// Find the servers for the languages in this folder, in the background.
    ///
    /// Discovery asks binaries their version, which means spawning processes: never on
    /// the frame thread, and never before there is a folder to search.
    pub(super) fn start_discovery(&mut self) {
        // The languages are a fact about the folder, so the panel can list them even
        // when the servers are turned off.
        self.lsp_path_dirty.clear();
        self.lsp_kinds = self
            .folder
            .as_ref()
            .map(|f| lsp::servers::kinds_among(f.files.iter().map(|e| e.path.as_path())))
            .unwrap_or_default();
        let root = self.folder.as_ref().map(|f| f.root.clone());
        // A server already reading *this* folder is still reading this folder: re-walking
        // the tree to hide or show a file is not a reason to make it start again. One
        // rooted anywhere else is another folder's server and is let go.
        let enabled = self.store.config.lsp.enabled;
        let mut kept = Vec::new();
        for mut session in std::mem::take(&mut self.servers) {
            if enabled && root.as_ref() == Some(&session.root) {
                kept.push(session);
            } else {
                session.stop();
            }
        }
        self.servers = kept;
        self.nav = None;
        self.available.clear();
        self.discovering = None;
        if !enabled {
            return;
        }
        let Some(root) = root else { return };
        let kinds = self.lsp_kinds.clone();
        if kinds.is_empty() {
            return;
        }
        self.discover(kinds, root);
    }

    /// Look for servers for these languages, off the frame thread: `find` runs the
    /// binaries it finds, which is not something a frame may wait for.
    pub(super) fn discover(&mut self, kinds: Vec<&'static lsp::servers::Kind>, root: PathBuf) {
        if kinds.is_empty() {
            return;
        }
        let overrides: Vec<Option<lsp::servers::Override>> = kinds
            .iter()
            .map(|kind| {
                self.store
                    .config
                    .lsp
                    .servers
                    .get(kind.id)
                    .map(|s| lsp::servers::Override {
                        command: s.command.clone(),
                        args: s.args.clone(),
                    })
            })
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let look_here = root.clone();
        std::thread::spawn(move || {
            let found = kinds
                .iter()
                .zip(overrides)
                .filter_map(|(kind, over)| lsp::servers::find(kind, &look_here, over.as_ref()))
                .collect();
            let _ = tx.send(found);
        });
        self.discovering = Some(Discovery { rx, root });
    }

    pub(super) fn poll_discovery(&mut self) {
        let Some(job) = &self.discovering else { return };
        let root = job.root.clone();
        match job.rx.try_recv() {
            Ok(found) => {
                self.discovering = None;
                // The reader may have opened another folder while this ran.
                if self.folder.as_ref().map(|f| f.root.clone()) == Some(root) {
                    // Merged rather than replaced: a restart looks for one language, and
                    // must not forget the servers the others already have.
                    for server in found {
                        self.available.retain(|a| a.kind.id != server.kind.id);
                        self.available.push(server);
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.discovering = None,
        }
    }

    /// Press `gd` at a place, for a test with nobody at the keyboard.
    ///
    /// `ZENITH_GOTO=LINE:COL` asks at that place in whatever file is open, then reports
    /// where it landed and exits. The language-server path is the one thing in the app
    /// that cannot be checked by rendering a frame: the question, the handshake and the
    /// arrival only happen when a key is pressed, and a key cannot be pressed headlessly.
    #[cfg(debug_assertions)]
    pub(super) fn dev_nav(&mut self, ctx: &egui::Context) {
        // A landing may leave nothing asking for another frame, and this hook can only
        // report what it sees; keep the frames coming while it is watching.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let Ok(spec) = std::env::var("ZENITH_GOTO") else {
            return;
        };
        // `ZENITH_GOTO_FROM=LINE` puts the reader somewhere first, so that a jump has a
        // direction to have. It is re-applied for a few frames because the page does not
        // know its own height on the first one.
        if let Some(line) = std::env::var("ZENITH_GOTO_FROM")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            if !self.nav_tried && self.frame < 40 {
                self.pending = Pending::Top(line.saturating_sub(1));
                return;
            }
        }
        // Fire once discovery has had its chance — or once it is clear it never will,
        // which is what happens when servers are turned off.
        if !self.nav_tried && (!self.available.is_empty() || self.frame > 90) {
            self.nav_tried = true;
            let mut parts = spec.rsplitn(3, ':');
            let col: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            let line: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            self.caret = vim::Caret::new(line.saturating_sub(1), col.saturating_sub(1));
            eprintln!(
                "nav: asking at {line}:{col} — {} server(s) available, {} running",
                self.available.len(),
                self.servers.len()
            );
            self.goto_definition();
            eprintln!("nav: after gd — status {:?}", self.status);
        }
        if self.nav_tried && self.nav.is_none() {
            // The page only acts on a landing when it draws, so let it draw first.
            self.nav_settled += 1;
            if self.nav_settled < 4 {
                return;
            }
            // `ZENITH_THEN=back|close` presses the key that undoes a jump, so that the
            // way back can be checked without a hand on the keyboard.
            if !self.then_done {
                self.then_done = true;
                match std::env::var("ZENITH_THEN").as_deref() {
                    Ok("back") => {
                        self.jump(-1);
                        eprintln!(
                            "nav: back to {} line {}",
                            self.open
                                .as_ref()
                                .map(|o| crate::catalog::display_path(&o.path))
                                .unwrap_or_default(),
                            self.caret.line + 1
                        );
                    }
                    Ok("close") => {
                        self.close_file();
                        eprintln!(
                            "nav: closed to {} line {}",
                            self.open
                                .as_ref()
                                .map(|o| crate::catalog::display_path(&o.path))
                                .unwrap_or_else(|| "(the reading room)".into()),
                            self.caret.line + 1
                        );
                    }
                    _ => {}
                }
                self.nav_settled = 0;
                return;
            }
            // Exit is not a shutdown here: the servers have to be let go first, or the
            // child processes outlive the test.
            for session in &mut self.servers {
                session.stop();
            }
            let outline: Vec<String> = self
                .open
                .as_ref()
                .map(|o| {
                    o.outline
                        .iter()
                        .take(4)
                        .map(|s| format!("{}:{}", s.name, s.line + 1))
                        .collect()
                })
                .unwrap_or_default();
            eprintln!(
                "nav: landed at line {} col {}, status {:?}",
                self.caret.line + 1,
                self.caret.col + 1,
                self.status
            );
            eprintln!(
                "nav: outline {} entries, first: {}",
                self.open.as_ref().map(|o| o.outline.len()).unwrap_or(0),
                outline.join(", ")
            );
            eprintln!(
                "nav: scroll {:.0} of a {:.0} viewport, page top line {}",
                self.scroll,
                self.view_h,
                self.open
                    .as_ref()
                    .and_then(|o| o.laid.as_ref())
                    .map(|l| l.line_at(self.scroll) + 1)
                    .unwrap_or(0)
            );
            std::process::exit(0);
        }
        if self.frame > 900 {
            eprintln!(
                "nav: gave up — status {:?}, nav {:?}, servers {:?}",
                self.status,
                self.nav.as_ref().map(|n| (n.id, n.tries)),
                self.servers
                    .iter()
                    .map(|s| (s.id(), format!("{:?}", s.state())))
                    .collect::<Vec<_>>()
            );
            std::process::exit(1);
        }
    }

    /// Where is the thing under the cursor defined?
    pub(super) fn goto_definition(&mut self) {
        let Some(open) = &self.open else {
            self.status = "gd: no file is open".into();
            return;
        };
        let path = open.path.clone();
        let line = self.caret.line;
        let unit = open
            .doc
            .lines
            .get(line)
            .map(|l| lsp::text::position(l, self.caret.col))
            .unwrap_or(0);
        if !self.store.config.lsp.enabled {
            self.status = "gd: language servers are off, in settings".into();
            return;
        }
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            self.status = "gd: no language server serves this kind of file".into();
            return;
        };
        if self.discovering.is_some() && self.available.is_empty() {
            self.status = format!("gd: still looking for a {} language server…", kind.title);
            return;
        }
        if !self.available.iter().any(|a| a.kind.id == kind.id) {
            self.status = format!(
                "gd: no {} language server on this machine — `zenith --lsp` says what it looks for",
                kind.title
            );
            return;
        }
        let root = self
            .folder
            .as_ref()
            .map(|f| f.root.clone())
            .unwrap_or_else(|| crate::catalog::project_root(&path));
        self.begin_nav(root, path, line, unit);
    }

    /// Start a server for the language being read, if one was found and is not running.
    ///
    /// This happens when a file of that language is opened, not when `gd` is pressed: the
    /// point of finding a server is that it starts reading the project while the reader
    /// reads, so that by the time a name needs following the answer is already there.
    pub(super) fn ensure_servers(&mut self) {
        if !self.store.config.lsp.enabled {
            return;
        }
        let Some(path) = self.open.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            return;
        };
        if self.servers.iter().any(|s| s.id() == kind.id) {
            return;
        }
        let Some(found) = self
            .available
            .iter()
            .find(|a| a.kind.id == kind.id)
            .cloned()
        else {
            return;
        };
        let root = self
            .folder
            .as_ref()
            .map(|f| f.root.clone())
            .unwrap_or_else(|| crate::catalog::project_root(&path));
        match lsp::Session::start(kind, &found, &root) {
            Ok(session) => self.servers.push(session),
            Err(e) => self.status = format!("language server: {e}"),
        }
    }

    /// Start the server if it is not running, then ask it the question.
    pub(super) fn begin_nav(&mut self, root: PathBuf, path: PathBuf, line: usize, unit: u32) {
        let Some((kind, _)) = lsp::servers::kind_for(&path) else {
            return;
        };
        if self.discovering.is_some() && self.available.is_empty() {
            self.status = format!("gd: still looking for a {} language server…", kind.title);
            return;
        }
        self.ensure_servers();
        if !self.servers.iter().any(|s| s.id() == kind.id) {
            self.status = format!("gd: no {} language server on this machine", kind.title);
            return;
        }
        self.status = format!("gd: asking {}…", kind.title);
        self.nav = Some(Nav {
            id: None,
            tries: 0,
            again_at: None,
            sent_at: None,
            path,
            line,
            unit,
            root,
        });
    }

    /// Send a waiting question once its server has finished its handshake.
    pub(super) fn advance_nav(&mut self) {
        let Some(nav) = &self.nav else { return };
        let (root, path) = (nav.root.clone(), nav.path.clone());
        let (line, unit) = (nav.line, nav.unit);
        let (id, sent, again) = (nav.id, nav.sent_at, nav.again_at);
        let kind = lsp::servers::kind_for(&path).map(|(k, _)| k.id);
        let Some(index) = self
            .servers
            .iter()
            .position(|s| s.root == root && Some(s.id()) == kind)
        else {
            // The server this question was waiting for is gone — it failed its handshake,
            // or the folder changed underneath it. Stop waiting: an unanswered question
            // must not keep asking for frames.
            self.nav = None;
            return;
        };

        // A question that has gone out is waited for, but not forever, and the waiting
        // is longest while the server is still reading the project — that wait is the
        // honest one, and the strip says so.
        if id.is_some() {
            let waited = sent.map(|t| t.elapsed()).unwrap_or_default();
            let idle = self.servers[index].is_idle();
            let limit = if idle {
                std::time::Duration::from_secs(5)
            } else {
                std::time::Duration::from_secs(25)
            };
            if waited > limit {
                let title = self.servers[index].kind.title;
                self.nav = None;
                if idle {
                    self.say("gd: the server did not answer");
                } else {
                    self.say(format!("gd: {title} is still reading this project"));
                }
            } else if !idle {
                let title = self.servers[index].kind.title;
                self.say(format!("gd: {title} is still reading this project…"));
            }
            return;
        }
        if again.is_some_and(|t| t > std::time::Instant::now()) {
            return;
        }
        if *self.servers[index].state() != lsp::State::Ready {
            return;
        }
        match self.servers[index].definition(&path, line as u32, unit) {
            Ok(id) => {
                if let Some(nav) = &mut self.nav {
                    nav.id = Some(id);
                    nav.sent_at = Some(std::time::Instant::now());
                }
            }
            Err(e) => {
                self.nav = None;
                self.say(format!("gd: {e}"));
            }
        }
    }

    /// Ask the server for the outline of the document being read, once.
    ///
    /// Until it answers — and if it never does — the panel shows what the grammar
    /// guessed, which is why nothing has to wait for a server to have a contents list.
    pub(super) fn ensure_outline(&mut self) {
        if !self.store.config.lsp.enabled {
            return;
        }
        let Some(path) = self.open.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        if self.outline_asked.as_deref() == Some(path.as_path()) {
            return;
        }
        let kind = lsp::servers::kind_for(&path).map(|(k, _)| k.id);
        let Some(session) = self
            .servers
            .iter_mut()
            .find(|s| *s.state() == lsp::State::Ready && Some(s.id()) == kind)
        else {
            return;
        };
        if session.outline(&path).is_ok() {
            self.outline_asked = Some(path);
        }
    }

    /// Take whatever the servers have said since the last frame.
    pub(super) fn poll_lsp(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        let asked = self.nav.as_ref().and_then(|n| n.id);
        let mut tries = 0u32;
        let mut land: Option<Vec<lsp::Target>> = None;
        let mut gone: Vec<String> = Vec::new();
        for session in &mut self.servers {
            for answer in session.poll() {
                match answer {
                    lsp::Answer::Ready => {}
                    lsp::Answer::Ended(why) => {
                        let mut line = format!("{} — {why}", session.kind.title);
                        if let Some(fix) = lsp::servers::advice(session.kind, &why) {
                            line.push_str(" · ");
                            line.push_str(&fix);
                        }
                        gone.push(line);
                    }
                    lsp::Answer::Found { id, targets } => {
                        if Some(id) != asked {
                            continue;
                        }
                        if targets.is_empty() && !session.is_idle() && tries < 12 {
                            // Ask again shortly: the server is reading the project, and
                            // "nothing is defined there" would be a lie about a file it
                            // has not opened yet.
                            tries += 1;
                            if let Some(nav) = &mut self.nav {
                                nav.id = None;
                                nav.tries += 1;
                                nav.again_at = Some(
                                    std::time::Instant::now()
                                        + std::time::Duration::from_millis(600),
                                );
                            }
                            self.status = format!(
                                "gd: {} is still reading this project…",
                                session.kind.title
                            );
                        } else {
                            land = Some(targets);
                        }
                    }
                    lsp::Answer::Refused { id, message } => {
                        if Some(id) == asked {
                            if lsp::transient(&message) && tries < 5 {
                                tries += 1;
                                if let Some(nav) = &mut self.nav {
                                    nav.id = None;
                                    nav.tries += 1;
                                    nav.again_at = Some(
                                        std::time::Instant::now()
                                            + std::time::Duration::from_millis(250),
                                    );
                                }
                                self.status =
                                    format!("gd: {} is still settling…", session.kind.title);
                            } else {
                                self.nav = None;
                                self.status = format!("gd: {message}");
                            }
                        }
                    }
                    lsp::Answer::Outline(symbols) => {
                        // A server's outline may arrive after the reader has moved on.
                        let waiting = self.outline_asked.clone();
                        if waiting.is_none() || !symbols.is_empty() {
                            if let (Some(open), Some(waiting)) = (&mut self.open, waiting) {
                                if open.path == waiting {
                                    open.outline = symbols
                                        .into_iter()
                                        .map(|s| Symbol {
                                            line: s.line,
                                            name: s.name,
                                            kind: s.kind,
                                            depth: s.depth,
                                            detail: s.detail,
                                        })
                                        .collect();
                                }
                            }
                        }
                    }
                }
            }
        }
        if !gone.is_empty() {
            self.status = format!("gd: {}", gone.join("; "));
            self.nav = None;
            self.servers
                .retain(|s| !matches!(s.state(), lsp::State::Failed(_)));
        }
        if let Some(targets) = land {
            self.nav = None;
            self.land(targets);
        }
    }

    /// Go where the server said. One target is a teleport; several are a page opening at
    /// the first of them, and the count says so rather than pretending there was one.
    pub(super) fn land(&mut self, targets: Vec<lsp::Target>) {
        let Some(first) = targets.first() else {
            self.status = "gd: nothing at that place".into();
            return;
        };
        let count = targets.len();
        let there = first.place();
        let path = first.path.clone();
        let (line, unit) = (first.line, first.unit);
        let here = self.open.as_ref().map(|o| o.path.clone());
        let left = self
            .open
            .as_ref()
            .map(|o| o.rel.clone())
            .unwrap_or_default();
        // Is the definition outside the folder being read? A dependency's source is
        // legitimate reading, but the reader has left the page they were on.
        let outside = self
            .folder
            .as_ref()
            .map(|f| !path.starts_with(&f.root))
            .unwrap_or(true);
        let previous = self.open.as_ref().map(|o| (o.path.clone(), self.scroll));
        let (previous, _) = (previous, self.scroll);
        if here.as_deref() != Some(path.as_path()) {
            self.push_jump();
            self.open_file_nav(&path);
        }
        let Some(open) = &self.open else { return };
        let line = line.min(open.doc.lines.len().saturating_sub(1));
        let col = open
            .doc
            .lines
            .get(line)
            .map(|l| lsp::text::column(l, unit))
            .unwrap_or(0);
        let caret = vim::Caret::new(line, col);
        if here.as_deref() == Some(path.as_path()) && caret != self.caret {
            self.push_jump();
        }
        // Following a definition is an arrival, not a nudge: the line comes to the top
        // of the page, which is where someone who has just arrived starts reading. Going
        // *backwards* is the exception — a line put at the top when it was above you
        // takes away the context you were reading, so it arrives in the middle instead,
        // with the code that leads into it still on screen.
        let above = match (&self.open, &previous) {
            (Some(open), Some((path, top))) if open.path == *path => {
                let first = open.laid.as_ref().map(|l| l.line_at(*top)).unwrap_or(0);
                caret.line < first
            }
            _ => false,
        };
        self.caret = caret;
        self.ensure_visible();
        self.pending = if above {
            Pending::Centre(caret.line)
        } else {
            Pending::Top(caret.line)
        };
        let mut said = if count > 1 {
            format!("→ {there} (1 of {count})")
        } else {
            format!("→ {there}")
        };
        if self.review.is_some() && self.review_return.is_some() {
            said.push_str(" · q back to the review");
        } else if outside && !left.is_empty() {
            // The way back is not visible from here: the tree does not hold this file,
            // and nothing on the page says which key undoes the jump.
            said.push_str(&format!(" · ctrl-o back to {left}"));
        }
        self.status = said;
    }
}
