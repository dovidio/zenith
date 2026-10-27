# zenith

> **Experimental, and written by an AI.** An early-stage prototype, not a product:
> expect rough edges, and expect the keybindings, settings and config format to change
> without notice.

**A reading room for source code.**

<https://dovidio.github.io/zenith/>

![zenith reading its own README](docs/self-portrait.jpg)

Zenith is a native code *reader* for projects where people only read code — reviewing,
onboarding, archaeology, study. It is not an editor with the editing removed: the whole
interface is built on the assumption that you will never type a character into it.

Built against egui/eframe 0.35. Developed and read on macOS and Linux; Windows is written
for but untested. Zero bundled assets: no fonts, no icons, no images. Requires rustc ≥ 1.92.

```sh
cargo run --release -- ~/code/some-project
```

Navigation is vim's, as far as a reader needs it: `j k` (or `ctrl-j` / `ctrl-k`), `w b e`,
`0 ^ $`, `{ }`, `%`, `gg G`, `H M L`, `zz zt zb`, `ctrl-d ctrl-u`, `[ ]` for chapters, `gd` to follow a name to
where it is defined, and `ctrl-o` / `ctrl-i` to go back and forward through everywhere you
have been. The page opens alone; a small mark at the top opens four doors — the index, the
contents, the launcher and settings. `tab` opens the index with the keyboard in it: `j k`
to move, `l` or `enter` to open a file or fold a folder, `h` to back out, `esc` to return
to the page. `⌘K` opens the launcher: one field for files, views and the page's own actions.
`⌘P` (or `ctrl-p`) keeps that sheet scoped to files; `/` (or `⌘F`) searches *inside the file*
you are reading. The launcher can toggle focus lens, soft wrap, line numbers, distilled
type and rendered Markdown, cycle the themes, change the type size, and take you to the
index or contents. Toggles carry their current state, and every keyed action carries its
key beside the name, so the launcher is where the shortcuts are learned; the complete map
lives in Settings › Keys, and `?` opens that page.

The look is a system, not a mood: the six palettes, the three voices, the shape of a
sheet and the things this program refuses are written down in [DESIGN.md](DESIGN.md).

---

## Why it does not look like your editor

An editor optimises for *changing* text: a caret, a gutter of line numbers, a rainbow of
token colours so you can spot mistakes, horizontal scrolling, a status bar full of build
state. A reader optimises for *understanding* text. Zenith makes five bets.

- **Indentation is not whitespace.** Leading whitespace is stripped out of the text and
  drawn as *whisper hairlines* in a gutter — one per nesting level, fading with depth. You
  feel the shape of a block instead of counting spaces.
- **Comments are prose.** They are the only part of a source file written in English, so
  they are the only part set in a reading face: a serif, italic, in the faintest ink. The
  `//` or `#` mark is left behind in faint monospace, like a printer's pilcrow.
- **Blank lines are compressed.** A blank line costs 46% of a line, so a file's paragraph
  rhythm survives without the page turning into a corridor.
- **Long lines wrap and declare themselves.** Every row that continues a wrapped line is
  marked with `↳` in the margin, so you are never lost mid-expression, and you never scroll
  sideways. The reading measure is a setting, not an accident of window size.
- **Attention is a gradient.** The *focus lens* (`f`) dims everything except the block you
  are reading — painted as a scrim over the paper, so it looks like the page coming forward
  rather than a change of colour in the text.

### Tones, not rainbow

Every editor colour scheme is a rainbow: six or seven hues at full saturation, competing
for attention. Terrible for reading. Zenith assigns each token class a position on a
single muted tonal ladder — the same low-chroma band, shifted only in hue and lightness.

| tone | what it means | how it reads |
|---|---|---|
| `Keyword` | `fn`, `struct`, `class`, `if` | the darkest ink on the page |
| `KeywordSoft` | `pub`, `async`, `unsafe`, lifetimes | the same word, spoken quietly |
| `Type` | `String`, `HashMap`, `str`, `u32` | slightly cool |
| `Call` | function and method names | slightly warm |
| `Constant` / `Number` / `Str` | literals | three neighbouring tones |
| `Meta` | `#[derive]`, `@decorator`, `println!` | amber, like a margin note |
| `Comment` / `Doc` | prose | the faintest ink, in the serif |
| `Operator` / `Punct` | `=`, `->`, brackets | nearly a whisper |

Casing is a signal too: a capitalised word the grammar left unclassified is a type, not an
identifier. That single heuristic is what lets you read an unfamiliar codebase and see
shapes instead of soup.

### Three voices

A code editor is set in one monospace face, everywhere, forever. Zenith sets **code** in a
monospace voice (SF Mono by default; Hack, Menlo, Monaco, PT Mono…), **prose** — comments,
titles, the colophon — in a serif (Iowan Old Style; Charter, Athelas, Palatino,
Baskerville, Georgia…), and **chrome** in a humanist sans (SF Pro; Helvetica Neue, Avenir
Next, Optima, Geneva…).

Nothing is bundled: it reads the typefaces already on the machine and measures the glyphs
source code depends on before trusting them. It measures their *x-heights* too, and sets
the prose larger than the code by exactly that ratio — a book serif and a coding face at
the same point size are not the same size to the eye, and the serif is the one that loses
(`zenith --fonts` prints the measurement). That check earns its keep — Apple's handsome
New York maps `_` in its character table and draws it 0.005 em thick, so `snake_case_name`
renders as `snake case name`, and the default prose voice is Iowan Old Style because of it.
`zenith --fonts` says why any face was refused.

### Markdown, rendered

A Markdown file is rendered as the document it describes rather than set as its source:
`## Notes` is a heading called *Notes*, larger and in the strongest ink; `**strong**` is
stronger; emphasis is the italic face; `` `code` `` is the monospace voice on a tint of the
paper. Quotations are italic on a band with a rule down their left edge, fenced blocks sit
on that band with their contents untouched — a `#` in a code block is not a heading — and
`---` is a hairline rather than three dashes. `_` between word characters is not emphasis,
so a document that mentions `snake_case_name` keeps its underscores. `~~struck~~` gets a
line through it, `- [x]` keeps its box, and a table is drawn as a grid: the column widths
measured across the whole block and every cell padded to its column, in the code face, so
the padding actually lines up. `x` toggles back to the source, because sometimes the source
is what you came to read.

### Distilled reading

`d` recedes what a human is not reading in a strongly typed language: the type annotation,
the modifiers, the punctuation only a compiler needs. It changes **ink, not geometry** — no
token is removed, so lines wrap exactly as the file does and nothing is silently hidden
from a search or a motion.

| shape | languages | example | what recedes |
|---|---|---|---|
| annotation after the name | Rust, TypeScript, Python, Kotlin | `let total: Vec<u8> = …` | from the `:` to the `=` |
| type before the name | Java, C#, C, C++, Dart | `Map<String, Object> m = …` | the type, including generics |
| type after `var`, or a field | Go | `var x int = 1`, `Name string` | the type, not the name |

The rule that matters most is the one that *refuses*: a colon also introduces a dictionary
entry, a struct-literal field, a slice bound and a ternary's else, and fading a **value** is
far worse than fading nothing, because the reader cannot tell that something is missing. So
a `:` inside brackets is left alone, a run containing a literal or a call is left alone, and
an `=`-less colon must positively look like a type (`balance: i64,` recedes, `balance:
total,` does not).

### Six themes

Each is a tonal ladder plus a page, named for something it actually resembles — a theme is
a place, not a colour swatch:

| theme | the thing | what carries it |
|---|---|---|
| **Lugano** | the lake at midday | pale stone paper, lake-blue keywords, cypress types, terracotta calls |
| **Maionese** | a jar of the stuff | butter paper, dark olive ink, yolk-gold accents, gherkin green |
| **Bibione** | sand, pine shade, the Adriatic | warm sand paper, deep-sea keywords, pine types, umbrella orange |
| **Kununurra** | red rock at dusk | warm rock ground, vermilion keywords, sandstone types, turquoise only as the accent |
| **Panittha** | temple gold, jade, teak | dark teak ground, gold-leaf keywords, jade types, lotus-pink numbers |
| **Newsprint** | cold type, cheap stock | cool grey, no warmth in it at all |

`1`–`6` pick one; the settings panel names them.

![the six themes](docs/themes.jpg)

---

## Opening a project

Point it at a folder — `zenith ~/code/some-project`, `⌘O`, or drag a folder onto the
window — and the index is that folder. A file works too, and the index shows the project
it belongs to. With no argument it reopens the last folder, or the one you are
standing in if that looks like a project, and otherwise it waits, quietly, for you to
choose.

The tree respects the repository's own `.gitignore`, skips build output and dependency
trees outright, and floats the doorways — README, manifests, entry points — to the top of
each folder. Opening a project puts you in front of its README, not the first file
alphabetically. Nothing is catalogued or remembered beyond the folder itself: there is no
collection to curate, no workspace to configure.

## Following the code

`gd` goes to the definition of the name under the cursor; `ctrl-o` comes back. That is the
whole of zenith's language-server support, and it is one question — *where is this
defined* — asked of a server that has to answer it. No completion, no diagnostics, no type
hints in the margin, no rename. A reader does not change anything, so the client never
tells a server the document has changed: it opens the file, asks, and closes it.

A definition is often not in the folder you are reading: it is in a dependency, or in the
standard library. Zenith follows it anyway — reading the standard library is legitimate
reading — and a line at the foot of the page says so, naming the file you came from and the
key that goes back. `ctrl-o` walks back through everywhere you have been, across files;
`q` puts the file down and returns to the one underneath it, which is what `:bd` does in
vim, in a program that has no command line to type it into.

It uses the servers already on the machine. Zenith does not download, bundle or install
them, and that is deliberate rather than unfinished: the server that matches a project is
the one the project already builds against — rust-analyzer from the same toolchain,
`tsserver` from the project's own TypeScript, gopls built with a Go that understands its
`go.mod`. A copy fetched from the network is the wrong copy surprisingly often, and an
installer would make a reader into a package manager. `zenith --lsp` reports what it found,
where, and what each binary said about itself.

A binary on `PATH` is not a server that works. On macOS, `~/.cargo/bin/rust-analyzer` can
be a rustup shim whose component is missing: it is there, it is executable, and it exits
with `Unknown binary 'rust-analyzer' in official toolchain`. So discovery is a table and a
search — including the directories a window opened from the Finder cannot see, because the
desktop hands it `/usr/bin:/bin:/usr/sbin:/sbin` — and the *handshake* is the verification.
When a server refuses, a line at the foot of the page quotes the server's own words,
which is usually the exact command that fixes it.

Starting one has consequences: analysing a project means running its build scripts. That
cost is paid without asking, so it is a *setting* rather than a ceremony: a server starts
by itself when you open a file of a language that has one — while you read, so the answer
is ready when you press `gd` — and this turns them all off:

```toml
# ~/.config/zenith/config.toml
[lsp]
enabled = false
```

…or the switch in the settings sheet (`s`), which also lists the languages in the folder,
what each one's server is doing, and a field to point at a binary yourself when discovery
guesses wrong. The sheet has a rail of sections rather than a tab bar: five of them, and
one needs room for a path.

Every question a server asks in return is answered "nothing": no capabilities registered,
no configuration supplied, no edit ever applied.

The contents drawer comes from the server too, when there is one. A grammar can say where
a declaration starts; only the language itself knows that a method belongs inside an
`impl` and a variant inside its `enum`, so the drawer indents them that way. With no server
the panel falls back to reading the grammar and nothing is missing from the page.

The handshake is not readiness. A server that has not finished reading the project answers
"nothing is defined there", which is true and useless, so the client watches the server's
own progress reports and asks again rather than repeating a lie — a line at the foot of the
page says *still reading this project* while it waits.

## It does not change your code

Zenith reads your project and writes nothing into it. Files are opened with
`std::fs::read`; there is no save, no lockfile, and no cache directory inside the
repository. The one thing it may start on your behalf is a language server, and only for a
folder you have said yes to (see above). Its own state — theme, typography, measure,
reading positions, bookmarks —
lives in `~/.config/zenith/`, and if that directory is not writable it degrades to an
in-memory session rather than failing. That is a property of the code, not a promise in a
README: read for a while, then diff the modification times of a copy and nothing will have
moved.

## Notes

- **Platforms.** Nothing is hard-coded to one. Zenith scans the font directories of the
  machine it is running on — macOS, Linux (`/usr/share/fonts`, `~/.local/share/fonts`,
  `~/.fonts`, …), Windows — and if nothing matches, a bare container or an unusual
  distribution, it says so once and sets the page in egui's bundled faces rather than
  failing to start. On Linux, `rfd` uses the XDG desktop portal for "open a folder", so a
  portal needs to be present for that one dialog; everything else works without it.
- **The window's own bar.** On macOS the window stays entirely the system's — frame,
  traffic lights, dragging, resizing, snapping — with only the title bar made transparent,
  so the theme shows through it. On Linux and Windows a window manager paints its own frame
  and cannot be asked for a colour, so zenith draws one, buttons included, and `⌘Q`/`Ctrl+Q`
  quits everywhere regardless.
- **Diagnostics.** There is a headless mode for almost everything: `--dump`, `--scopes`,
  `--md`, `--distill`, `--fonts`, `--faces`, `--glyphs` (which mark the fonts here can
  actually draw), `--walk` (what a folder cost), `--lsp` (which language servers were found
  and what each said about itself), `--lsp-ask FILE:LINE:COL` (the handshake, a real
  question, and the answer, without a window), and `--smoke` (renders the app to a PNG,
  which is how the screenshots above were taken).
- **How it was written.** The code, the tests, this README and the screenshots were
  produced by an AI agent working from a human's direction. No human has read every line —
  treat it as a prototype, and read the code before you trust it with something you care
  about.

## Licence

Released under the [MIT licence](LICENSE-MIT).
