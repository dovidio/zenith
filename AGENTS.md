# AGENTS.md

Working notes for agents and contributors. The public entry point is [README.md](README.md);
this file carries the detail behind it.

## What this is

Zenith is a native, read-only code reader for senior engineers who have to understand
code they did not write — a review, an inherited service, a dependency, a standard-library
definition. It is not an editor with the editing removed. There is no save, rename,
format, diagnostics or caret to type with.

Read-only is structural: a feature that needs a write path into the project is not Zenith.
The page is the product; chrome is summoned and dismissed. The reader should be able to
spend an hour with no permanent control on screen.

The project is a public MIT prototype at [dovidio/zenith](https://github.com/dovidio/zenith)
and [dovidio.github.io/zenith](https://dovidio.github.io/zenith/). It is framed as
experimental and written by an AI; do not remove that disclosure without the author's
decision.

## Repository map

| path | owns |
|---|---|
| `zenith-core/` | the reading model: document, syntax, tone, motions, folder walk, review, outline, language servers |
| `src/app/` | the window's feature modules: review, sheets, tree, page, LSP UI, launcher, widgets, smoke |
| `src/app.rs` | `Zenith` state, startup, keyboard/actions, frame assembly |
| `src/reader.rs`, `src/theme.rs`, `src/typography.rs` | layout/painting, palettes, font discovery |
| `docs/` | the public site and its screenshots |
| `DESIGN.md` | the visual and interaction system; the authority for UI decisions |
| `.githooks/pre-commit` | the local CI: fmt, clippy and the whole test suite |
| `clippy.toml` | the smell thresholds |

`zenith-core` must not depend on egui. Verify with:

```sh
cargo tree -p zenith-core | grep -E 'egui|eframe'
```

No matches is the expected result. A TUI would depend on `zenith-core` only.

The window's modules are feature-shaped, not layered: `app/review.rs` owns the review
surface, `app/sheets.rs` the launcher/find/settings, `app/tree.rs` the index and contents,
`app/page.rs` the reading page, `app/lsp_ui.rs` server orchestration. State groups live
beside their features (`ReviewState`, `TreeState`, `Sheets`, `LspState`, `BloomState`).

## Commands

```sh
cargo run --release -- ~/code/some-project
cargo run -- --smoke /tmp/zenith.png
```

The pre-commit hook is the CI — there is no remote CI:

```sh
git config core.hooksPath .githooks
```

It runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
-- -D warnings`, and `cargo test --workspace --locked`. `--no-verify` is the escape hatch
for a deliberately unfinished commit. Do not commit a lowered threshold to make a gate
pass; fix the function or ask.

Headless diagnostics, useful without a window:

```sh
cargo run -- --dump FILE          # tones, outline and blank-line rhythm
cargo run -- --scopes FILE         # grammar scopes behind each tone
cargo run -- --md FILE             # how a Markdown file becomes a page
cargo run -- --distill FILE        # which tokens distilled reading would recede
cargo run -- --fonts               # available voices and why faces were refused
cargo run -- --faces FILE          # faces inside a font file and their coverage
cargo run -- --glyphs              # which interface marks this machine can draw
cargo run -- --walk FOLDER         # what a folder walk cost
cargo run -- --lsp [FOLDER]        # which servers were found and what they said
cargo run -- --lsp-ask FILE:L:C    # the LSP handshake and a definition, without a window
```

Screenshots come from the app, not a browser:

```sh
ZENITH_CONFIG_DIR=/tmp/zenith-shot-config \
  cargo run -- --smoke /tmp/zenith.png

ZENITH_CONFIG_DIR=/tmp/zenith-shot-config \
ZENITH_FORCE_REVIEW=1 ZENITH_REVIEW_HOVER=16 \
  cargo run -- --smoke /tmp/review.png /tmp/review-fixture
```

`ZENITH_CONFIG_DIR` keeps captures from touching the reader's real settings. Useful test
hooks: `ZENITH_FORCE_REVIEW`, `ZENITH_REVIEW_BASE_SHEET`, `ZENITH_REVIEW_INDEX`,
`ZENITH_REVIEW_BASE`, `ZENITH_PALETTE_QUERY`, `ZENITH_FORCE_PALETTE`, `ZENITH_FORCE_SETTINGS`,
`ZENITH_FORCE_TREE`, `ZENITH_FORCE_OUTLINE`, `ZENITH_CARET`, `ZENITH_GOTO`.
The smoke harness writes two phases: the page, and the focus lens.

## Invariants

- **No writes into a repository.** Files are read with `std::fs::read`. Git commands set
  `GIT_OPTIONAL_LOCKS=0` and only diff/show/ls-files. The only process Zenith may start is a
  language server, for a folder the reader opened.
- **State outside the repository.** Theme, typography, measure, reading positions and
  bookmarks live in `~/.config/zenith`; if that is not writable, the session stays in memory.
- **No bundled assets.** Fonts, servers and screenshots are the machine's and the app's;
  no font, icon set or image ships inside the binary.
- **Core stays UI-free.** `zenith-core` may not import egui; semantic `Tone` is the seam.
  The window decides ink; core decides what the ink is.
- **No editor furniture.** No permanent sidebar, status bar, toolbar, minimap, tabs,
  rainbow syntax, diagnostics-as-decoration, green/red diffs or hand cursor.
- **The design system wins.** Read `DESIGN.md` before changing a surface; the palettes,
  three voices, sheet radii, bloom timing and refusal list are deliberate.

Lint gates: cognitive complexity ≤ **25**, function length ≤ **250**. The thresholds are in
`clippy.toml`; `Cargo.toml` denies the lints workspace-wide. If a function crosses one,
split it rather than adding an allow. `too_many_lines` is deliberately generous because
cognitive complexity is the sharper signal.

## Reading page

- Code, comments and chrome use three voices. Fonts are discovered from the machine,
  validated against the same parser egui rasterises with, and measured for x-height; prose
  is set larger than code by that ratio. Nothing is bundled.
- Tokens are semantic tones on one muted tonal ladder. Keywords are the darkest ink,
  comments the faintest; punctuation is dimmed. There are six complete themes, three
  daylight and three lamplit; each is a place, not a hue rotation.
- Indentation is drawn as fading hairlines, not typed. Comments are set in prose. Blank
  lines are compressed. Long lines wrap with a `↳` mark. The cursor is a ribbon, a wash and
  a column hairline, not a blinking caret.
- Markdown renders as a document (`x` toggles source). Fences and quotations get a flat
  ink band, headings are prose, tables are measured grids.
- Distilled reading fades type annotations, modifiers and punctuation without removing
  tokens or changing geometry.
- The bloom is one accent dot with four words; it unfurls on intent, folds after 2.6 s,
  fades on first scroll and never floats over code. In review it has its own band.
- Settings live in a centered sheet with a five-section rail; every control is live and
  there is no Apply. `restore defaults` asks twice.

## Review

Review is a reading state, not a diff view. `g r` toggles it; `g b` opens the base sheet.
Bases: working tree (default), staged index, last commit, or a branch/commit typed into the
sheet. A branch is compared from its merge base with `HEAD`, not from the tip.

- Whole changed files, one page at a time. No hunks, no side-by-side, no green/red. A
  changed line is set in a real bold face; the accent margin carries a tab. A deletion is a
  hairline because it has no new line. The spine ticks every change.
- Hover a changed line, or press `p`, to lift the code it replaced in a paper sheet. The
  page does not move.
- `J K` walks changed files; `[ ]` walks changes. `J K` in a review is files even when the
  tree has focus; `[ ]` is changes.
- `q` from the review page leaves the review. `q` from a definition detour returns to the
  exact review page and caret, keeping the review. `ctrl-o`/`ctrl-i` remain generic history.
- Untracked source is included. The repository's `.gitignore` and Zenith's noise list
  (including dependency trees) are not. Binary or unreadable files stay in the strip with a
  notice.
- `r` re-reads the open file and rescans the folder in the background. The page, caret,
  review and expanded folders stay put. The index is a snapshot, not a watcher.

## Language servers

- Discovery is passive: a table plus a search of known locations. Zenith never downloads,
  bundles or installs a server. A binary on PATH is not a working server; the handshake is
  the verification.
- One question only: definition (`gd`), plus document symbols for the contents drawer. No
  completion, diagnostics, hover, edits or didChange notifications.
- Servers start implicitly for a language in the folder unless `[lsp] enabled = false` in
  `~/.config/zenith/config.toml` or the Settings › Language switch is off.
- A server's own error text is shown at the foot of the page. Readiness is tracked through
  progress reports; a server that is still indexing is asked again rather than trusted.
- `server_state` / `restart_server` own the path fields. A typed path restarts that
  language's server when the field is left, not on every keystroke.

## Docs and the site

- `README.md` is intentionally short: what it is, two screenshots, run, review, reading,
  and links. Put detail here or in `DESIGN.md`, not back into the README.
- `docs/index.html` and `docs/site.css` are the public landing page. It is a reading
  specimen, not a product page with cards: one sheet, a specimen listing, notes, themes,
  run, the room, review, trust and a close. Keep it within the existing world.
- Screenshots in `docs/` come from `--smoke`. Regenerate `self-portrait.jpg`,
  `reading-detail.jpg`, `review-detail.jpg`, `review.jpg` and `themes.jpg` when the page
  changes enough to make them stale.

## Gotchas

- `app.rs` is the module root for `src/app/*`; a child module may access parent private
  items, but methods called from the parent need `pub(super)`.
- Feature modules are children, so `use super::*` brings the window's imports; core imports
  are explicit `zenith_core::...`.
- `ReviewState`, `TreeState`, `Sheets`, `LspState`, `BloomState` are state groups, not
  widgets. Keep the state beside its methods.
- Macro-heavy formatting in `typography.rs` has `#[rustfmt::skip]` on the typeface ranking;
  do not let rustfmt turn one face per line into six.
- The smoke harness is not an assertion suite. Tests live beside the domain logic; UI
  changes still need a rendered look.
