# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Senior engineers reading unfamiliar code: code review, onboarding, archaeology and study.
Their job is architectural understanding — seeing the shape of a system and following a
name to its definition — not editing. Technical writers and learners may use it, but they
are not the design target.

## Product Purpose

Zenith is a native, read-only code reader — a reading room for source code. It exists so
an engineer can understand a codebase without the editor's editing affordances,
diagnostics and chrome. As an open-source tool, success is adoption by working engineers
who read code for a living: the measure is whether they reach for Zenith to read an
unfamiliar project and stay on the page, not whether it replaces their editor for writing.

## Positioning

Read-only is structural, not a mode: there is no write path into the project, so an
editor's caret, save, rename and diagnostics cannot exist by accident. Language servers
are used for one question — where is this defined — and for the document outline; never
for completion, diagnostics, hints or edits. The app uses the fonts and servers already on
the machine rather than bundling or downloading them. The alternative it offers is not a
lighter editor; it is a different reading surface.

## Operating Context

- Native desktop app written in Rust with egui/eframe, not web or mobile. The `adaptive`
  platform record means window chrome and type come from the host OS; macOS and Linux are
  primary, Windows is written for but untested.
- Readers open local folders or files (path, `⌘O`, drag), often a Git repository with its
  README and manifests. Reading works offline; language servers may run locally when
  available.
- Navigation is vim-like for reading (`j k`, `gg G`, `gd`, `ctrl-o` / `ctrl-i`). The
  program is pointedly not modal and has no command line.
- Configuration and reading memory live outside the repository, in `~/.config/zenith`; if
  that directory is not writable, the session runs in memory instead.
- The project is a public, MIT-licensed prototype: `dovidio/zenith`,
  `dovidio/hugo-zenith`, `dovidio/personalwebsite`; the site is
  `https://dovidio.github.io/zenith/`.
- It is publicly framed as experimental and written by an AI; the README warns that no
  human has read every line and that it is not production-ready.

## Capabilities and Constraints

- Reads source files and Markdown; no editing, saving, formatting, rename or code actions.
- Reviews a branch's git changes read-only: the working tree by default, or a branch,
  commit, staged index or the last commit; a branch is compared from its merge base.
  Changed lines are set in a heavier face and the code they replaced lifts in place.
  Untracked source is included; ignored noise, including dependency trees, is not.
- Language-server support is discovery and navigation only: definition and document
  symbols; no diagnostics, completion, hover or edits. Servers start implicitly unless
  disabled in settings, and are never downloaded, bundled or installed.
- The reader controls appearance and page settings (theme, measure, leading, type size,
  wrapping, blank-line compression, indentation guides, line numbers, focus lens and
  distilled reading); Markdown can be rendered or shown as source.
- Zero bundled assets: no fonts, icons or images ship with the app; fonts and servers come
  from the machine.
- Writes nothing into a repository. Its only possible side effects outside
  `~/.config/zenith` are starting a language server, which analyses the project, and
  running read-only git commands to compute a review.
- Reads the repository's `.gitignore`, skips build output and dependency trees, and
  surfaces READMEs, manifests and entry points as doorways.
- Undecided: a formal accessibility standard; distribution and packaging beyond a Cargo
  build and the public repository; whether Windows becomes a tested, first-class target.

## Brand Commitments

- Name: **Zenith**. Descriptor: **a reading room for source code**; the design system is
  called *Zenith Editorial Code*.
- Voice: literary, precise, unhurried, anti-IDE; candid about the prototype's limits.
- MIT licence; public GitHub account `dovidio`; live site `dovidio.github.io/zenith`.
- No logo, icon set, bundled type or brand imagery; the name and the page carry the
  identity.
- The README's "Experimental, and written by an AI" disclosure is a current commitment; do
  not remove or soften it without the user's decision.

## Evidence on Hand

- `README.md` — product description, navigation, language-server behaviour, promises about
  writes, platform notes.
- `DESIGN.md` — the intended interaction and visual system; a specification, not proof of
  implementation.
- `docs/index.html`, `docs/site.css`, `docs/zenith.css` — the public site.
- `docs/self-portrait.jpg`, `docs/reading-detail.jpg`, `docs/review-detail.jpg`,
  `docs/review.jpg`, `docs/themes.jpg` — app screenshots produced by `--smoke`.
- Source and tests under `src/`, including headless diagnostics (`--dump`, `--fonts`,
  `--lsp`, `--smoke`).
- No customer testimonials, benchmarks, case studies, pricing, press or deployment
  evidence exists; future work must not invent them.

## Product Principles

1. **Read-only is structural.** If a feature needs a write path into the project, it is not
   Zenith.
2. **The page is the product.** Chrome, drawers and sheets exist to return the reader to
   the text; nothing earns permanent screen space.
3. **Read code for understanding, not for editing.** Hierarchy should come from how a
   reader reads, not from the editor's diagnostics and build state.
4. **Use what the machine already has.** Fonts, language servers and project conventions
   are discovered, not bundled, downloaded or imposed.
5. **Calm over conventional discoverability.** The reading state stays quiet; capability
   appears as doors when asked for, not as a toolbar or a shortcut legend.

## Accessibility & Inclusion

No formal standard has been set. Current evidence: the reader can scale type up to 1.8× and
adjust measure and leading; reading and navigation are keyboard-driven. No accessibility
audit has been performed, no screen-reader path has been verified, and no reduced-motion
setting exists yet; none of that should be claimed as done.
