//! The screenshot harness: frames, phases, and the PNG it leaves behind.
//!
//! This is a development instrument, not a feature. It exists so a change of light can
//! be looked at rather than argued about.

use super::*;

impl Zenith {
    // -- smoke -------------------------------------------------------------

    /// A message in the strip, for photographing it: `ZENITH_FORCE_NOTICE="…"`.
    ///
    /// The status strip is the one piece of chrome whose job is to be read at a glance,
    /// and a glance is what a screenshot is for.
    #[cfg(debug_assertions)]
    pub(super) fn forced_notice(&mut self) {
        let Ok(text) = std::env::var("ZENITH_FORCE_NOTICE") else {
            return;
        };
        if self.frame <= 2 {
            self.status = text;
        }
    }

    pub(super) fn smoke_tick(&mut self, ctx: &egui::Context) {
        if self.smoke.is_none() {
            return;
        }
        // A self-portrait has to be reproducible, so each phase states its own
        // conditions explicitly rather than inheriting whatever was last saved.
        if self.smoke.as_ref().map(|s| s.frame) == Some(0) {
            match self.smoke.as_ref().map(|s| s.phase) {
                Some(0) => {
                    self.store.config.focus_lens = false;
                    self.store.config.distilled = false;
                    self.caret = vim::Caret::default();
                    self.pending = Pending::Restore;
                    // `ZENITH_CARET=LINE:COL` puts the cursor somewhere specific, for
                    // photographing what it looks like there.
                    if let Some(place) = std::env::var("ZENITH_CARET").ok().and_then(|v| {
                        let (l, c) = v.split_once(':')?;
                        Some((l.parse::<usize>().ok()?, c.parse::<usize>().ok()?))
                    }) {
                        self.caret =
                            vim::Caret::new(place.0.saturating_sub(1), place.1.saturating_sub(1));
                        self.ensure_visible();
                    }
                }
                _ => {
                    self.store.config.focus_lens = true;
                    let nested = self
                        .folder
                        .as_ref()
                        .map(|s| s.root.join("src/reader.rs"))
                        .filter(|p| p.is_file());
                    if let Some(path) = nested {
                        self.open_file(&path);
                    }
                    self.caret = vim::Caret::new((self.line_count() / 2).max(1), 0);
                    self.centre_on_caret();
                }
            }
        }
        let Some(smoke) = &mut self.smoke else { return };
        smoke.frame += 1;
        // Ask for the next frame on a timer rather than spinning: the renderer needs an
        // idle moment between frames to service a screenshot request, and a frame loop
        // that never idles drops them.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // Wait for the layout to settle before asking for a picture.
        // Ask for the picture, and ask again if the answer does not come. The renderer
        // drops the occasional screenshot request (it races with a frame that is already
        // in flight), and one dropped request used to mean a failed run and no images.
        if !smoke.requested || (smoke.frame - smoke.asked_at > 20 && smoke.asked < 12) {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
            smoke.requested = true;
            smoke.asked += 1;
            smoke.asked_at = smoke.frame;
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else {
            if smoke.frame > 600 {
                eprintln!("zenith: smoke — no screenshot arrived from the renderer");
                std::process::exit(3);
            }
            return;
        };

        let [w, h] = image.size;
        let mut rgb = Vec::with_capacity(w * h * 3);
        for px in &image.pixels {
            let [r, g, b, _a] = px.to_array();
            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
        let out = match smoke.phase {
            0 => smoke.out.clone(),
            _ => {
                let mut p = smoke.out.clone();
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "smoke".into());
                p.set_file_name(format!("{stem}-zen.png"));
                p
            }
        };
        if let Err(e) = std::fs::write(&out, crate::png::encode_rgb(w as u32, h as u32, &rgb)) {
            eprintln!("zenith: smoke — could not write {out:?}: {e}");
            std::process::exit(4);
        }
        println!("zenith: smoke — wrote {} ({w}×{h})", out.display());

        match smoke.phase {
            0 => {
                // Phase two: the focus lens, reading zenith's own layout engine — a
                // file with real nesting, so the lens has work to do.
                smoke.phase = 1;
                smoke.requested = false;
                smoke.asked = 0;
                smoke.asked_at = 0;
                smoke.frame = 0;
                self.store.config.wrap = true;
            }
            _ => {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }
}
