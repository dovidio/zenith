# zenith

> **Experimental, and written by an AI.** An early-stage prototype, not a product:
> expect rough edges, and expect the keybindings, settings and config format to change
> without notice.

**A reading room for source code.**

![zenith reading its own README](docs/self-portrait.jpg)

Zenith is a native code *reader* for projects where people only read code — reviewing,
onboarding, archaeology, study. It is not an editor with the editing removed: the whole
interface is built on the assumption that you will never type a character into it.

```
zenith ~/hack/zenith          # open a project
zenith --fonts                # what typefaces are available
zenith --dump src/main.rs     # parse a file headlessly and print its tones
zenith --scopes src/main.rs 8 # show the grammar scopes behind each tone
zenith --grammars             # list the 72 grammars it can set text in
```

Navigation is vim's, as far as a reader needs it: `j k`, `w b e`, `0 ^ $`, `{ }`, `%`,
`gg G`, `H M L`, `zz zt zb`, `ctrl-d ctrl-u`, `[ ]` for chapters, and `ctrl-o` / `ctrl-i`
to go back and forward through everywhere you have been. `gd` is reserved for *go to
definition*.

There are two finders and they are deliberately separate: **`⌘P`** (or `⌘K`, or `ctrl-p`)
finds *a file* in the shelf, and **`/`** (or `⌘F`) searches *inside the file* you are
reading, with `n` and `N` to walk the matches. Opening either closes the other. The status
bar names both.

Press `?` inside the app for the whole map, `Z` for zen mode, `s` for settings.

---

## Why it does not look like your editor

An editor optimises for *changing* text: a caret, a gutter of line numbers, a rainbow of
token colours so you can spot mistakes, horizontal scrolling, a status bar full of build
state. A reader optimises for *understanding* text. Zenith makes five different bets.

**1. Indentation is not whitespace.** Leading whitespace is stripped out of the text and
drawn as *whisper hairlines* in a gutter — one hairline per nesting level, fading with
depth. You feel the shape of a block instead of counting spaces. The deepest guides are
the faintest, because that is how attention works.

**2. Comments are prose.** Comments are the only part of a source file written in
English, so they are the only part set in a reading face: a serif, italic, in the
faintest ink on the page. The `//` or `#` mark is left behind in faint monospace as a
marginal sign, like a printer's pilcrow. Doc comments get slightly more air than code
because they are addressed to you.

**3. Blank lines are compressed.** A blank line costs 46% of a line. A file's paragraph
rhythm survives without the page turning into a corridor.

**4. Long lines wrap and declare themselves.** Every row that continues a wrapped line is
marked with `↳` in the margin, so you are never lost mid-expression, and you never scroll
sideways. The reading measure (line length) is a setting, not an accident of window size.

**5. Attention is a gradient.** The *focus lens* (`f`) dims everything except the block
you are reading. Dimming is painted as a scrim over the paper rather than as a colour
change in the text, so the fade literally looks like the page coming forward — and the
layout cache survives unbroken. Only the visible lines are laid out per frame, so the cost
of a frame does not grow with the size of the file.

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

A code editor is set in one monospace face, everywhere, forever. Zenith sets:

- **code** in a monospace voice (SF Mono by default; Hack, Menlo, Monaco, PT Mono…),
- **prose** (comments, titles, the colophon) in a serif voice (Iowan Old Style by
  default; Charter, Athelas, Palatino, Baskerville, Georgia…),
- **chrome** in a humanist sans (SF Pro; Helvetica Neue, Avenir Next, Optima, Geneva…).

Nothing is bundled. Zenith reads the typefaces already installed on the machine and
validates each one with `skrifa` — the *same parser egui rasterises with* — and then goes
further than parsing: it measures the glyphs that source code depends on.

That check earns its keep. New York, Apple's handsome screen serif, maps `_` in its
character table and draws it **0.005 em thick** — a fifth of a pixel at a 15 pt reading
size, so it rounds away to nothing and `snake_case_name` is rendered as `snake case name`.
No other face on this machine is thinner than 0.047 em. The default prose voice is
therefore Iowan Old Style, and `zenith --fonts` will tell you exactly why any face was
refused (`--faces FILE` does the same for every face in a collection, with its italic,
weight, and coverage).

The rule follows the *document*, not the extension. A Markdown file is prose, so its body
and headings are set in the serif while its fenced code blocks stay monospace and its
`**emphasis**` and `` `code` `` markers are demoted to faint marginal signs. A source file
is code, so only its comments get the reading face.

### Reading something enormous

A file is read in two passes. The first screenful is toned immediately — reading is what
the reader came to do — and the rest of the file arrives as plain text with its tones
following in the background, so search, motions, wrapping and scrolling all work from the
first frame. Opening a 4.7 MB source file costs about half a second to first paint instead
of six seconds of a frozen window, and the tones land a few seconds later without anyone
waiting for them.

Folding a folder works the same way, and for the same reason: reading a directory tree
means a syscall per file, so the walk happens off the frame path and the tree fills in
when it arrives.

### Distilled reading

`d` recedes what a human is not reading in a strongly typed language: the type
annotation, the modifiers, and the punctuation only a compiler needs — the trailing `;`
in particular. It changes **ink, not geometry**: no token is removed, so lines wrap
exactly as the file does and every column stays where the source put it, which means
nothing is silently hidden from a search or a motion.

The rule that matters most is the one that *refuses*. A colon introduces a type in Rust,
a dictionary entry in Python, a struct-literal field in Rust, a slice bound, a lambda
body and a ternary's else in half the languages here — and fading a **value** is far
worse than fading nothing, because the reader cannot tell that something is missing. So
the distiller is per language, and it errs toward refusing:

| shape | languages | example | what recedes |
|---|---|---|---|
| annotation after the name | Rust, TypeScript, Python, Kotlin | `let total: Vec<u8> = …` | from the `:` to the `=` |
| type before the name | Java, C#, C, C++, Dart | `Map<String, Object> m = …` | the type, including generics |
| type after `var`, or a field | Go | `var x int = 1`, `Name string` | the type, not the name |

Everything else is refused: a `:` inside brackets, a `?` ternary before it, a run
containing a literal, a call or a value operator, and any `=`-less colon whose right side
does not positively look like a type (`balance: i64,` recedes, `balance: total,` does not).
Language-agnostic and always applied: a trailing `;` goes invisible, punctuation and
operators lose 45% of their ink, and modifiers (`pub`, `static`) lose 30%.

Two limits. **Parameter and return types are not touched** (`fn f(a: i32) -> T`,
`def f(a: int) -> str:`, `void loop(List<String> l)`) — they live inside brackets or after
an arrow, where the shape is ambiguous with a call — and **grammars zenith does not have**
(Kotlin and Swift are set with borrowed grammars, and TypeScript with the JavaScript one)
distil only as well as those grammars tokenise. `zenith --distill FILE` prints exactly
what would recede, per file:

```sh
$ zenith --distill Ledger.java
Ledger.java — grammar Java, distillation: Prefix
    2  [String] name = "x";
    3  public final [int] count = 0;
    4  [Map][<][String][,][ ][Object][>] m = new HashMap<>();
    5  [int] pick = flag ? 1 : 2;      # the ternary's else is untouched
```

Going beyond this — moving types out of signatures, eliding redundant parentheses,
restructuring a declaration into prose — is a language-aware *rendering* project rather
than a reading mode, and it needs precision zenith does not have yet: the language
server's semantic tokens, which say exactly which span is a type rather than zenith
inferring it. That is the same step that unlocks `gd`.

### Six themes

Each is a tonal ladder plus a page, and each is named for something it actually
resembles — a theme is a place, not a colour swatch:

| theme | the thing | what carries it |
|---|---|---|
| **Lugano** | the lake at midday | pale stone paper, lake-blue keywords, cypress types, terracotta calls |
| **Maionese** | a jar of the stuff | butter paper, dark olive ink, yolk-gold accents, gherkin green |
| **Bibione** | sand, pine shade, the Adriatic | warm sand paper, deep-sea keywords, pine types, umbrella orange |
| **Kununurra** | pindan red earth and the Ord | red-brown ground, turquoise keywords, sand ochre types, spinifex green |
| **Panittha** | temple gold, jade, teak | dark teak ground, gold-leaf keywords, jade types, lotus-pink numbers |
| **Newsprint** | cold type, cheap stock | cool grey, no warmth in it at all |

`1`–`6` pick one; the settings panel names them.

![the six themes](docs/themes.jpg)

---

## The team shelf

Zenith is built for reading *a team's* work, so it opens onto a catalogue rather than a
file dialog:

- **Team** — every project under your workspace root (defaults to the first of
  `~/hack`, `~/projects`, `~/code`, `~/dev`, `~/src`, `~/work` that exists), discovered
  from repository/manifest markers and ordered by most recently touched — a team's
  attention is usually what you want to follow.
- **Shelves** — any folder you name by hand in the config.
- **Open a folder…** (`⌘O`, or drag a folder onto the window) for anything else.

Inside a project, the file tree respects the repository's own `.gitignore`, skips build
output and dependency trees outright, and floats the doorways — README, manifests, entry
points — to the top of each folder. On opening, a reader is put in front of the README,
not the first file alphabetically.

## It does not change your code

Zenith reads your project and writes nothing into it. Files are opened with
`std::fs::read`; there is no save, no lockfile, and no cache directory inside the
repository. Its own state — theme, typography, measure, reading positions, bookmarks —
lives in `~/.config/zenith/` (override with `ZENITH_CONFIG_DIR`), and if that directory
is not writable it degrades to an in-memory session instead of failing.

Nothing is hidden inside the project to make this true: it is a property of the code, and
[the check below](#verification) is how you confirm it on your own repository.

## Verification

- `cargo test` — the vim motions, the distiller, tone mapping, typography selection, the
  fuzzy matcher and the PNG writer.
- `zenith --dump FILE` — the full parse pipeline, headlessly: outline, tones, rhythm.
- `zenith --scopes FILE N` — the grammar scopes behind every tone.
- `zenith --fonts` / `zenith --faces FILE` — what each face can actually draw, and why a
  face was refused.
- `zenith --distill FILE` — which tokens distilled mode would recede, and which it
  refuses, per file.
- `zenith --glyphs` — which of the marks the interface draws (triangles, diamonds, the
  bookmark star) this machine's fonts can actually render, and what is used instead when
  they cannot. Apple's fonts have all of them; a bare Linux install may have none, and a
  mark that is missing would otherwise be a tofu box.
- `zenith --walk FOLDER` — walk a folder the way the shelf does and report what it cost,
  how many files were filtered and why that count is not a loss.
- `zenith --smoke OUT.png` — renders the app, saves a self-portrait of both the normal
  view and zen mode with the focus lens, then exits. The screenshots in `docs/` were
  produced by it, and are written as uncompressed PNGs by `src/png.rs` (zenith links no
  image libraries).
- To check the read-only claim yourself:

  ```sh
  cp -R project /tmp/probe
  find /tmp/probe -exec stat -f "%m %N" {} \; | sort > /tmp/before
  zenith /tmp/probe &            # read for a while, then quit
  find /tmp/probe -exec stat -f "%m %N" {} \; | sort > /tmp/after
  diff /tmp/before /tmp/after    # silence: not one mtime moved
  ```

  Against a 13-file copy of this repository, the file list and every modification time
  were identical afterwards, and nothing was created inside the project.

## Platforms

Nothing is hard-coded to a platform. Zenith scans the font directories of the machine it
is running on — macOS (`/System/Library/Fonts`, `~/Library/Fonts`), Linux
(`/usr/share/fonts`, `/usr/local/share/fonts`, `~/.local/share/fonts`, `~/.fonts`,
`/run/host/fonts`), Windows (`%SystemRoot%\Fonts`) — and matches what it finds against a
preference list that names the usual faces on each (JetBrains Mono, Fira Code, DejaVu Sans
Mono, Liberation Mono, Noto Sans Mono, Ubuntu Mono on Linux; SF Mono and Menlo on macOS).
If nothing matches — a bare container, an unusual distribution — it says so once and sets
the page in egui's bundled faces rather than failing to start. `ZENITH_NO_SYSTEM_FONTS=1`
forces that path.

On Linux, `rfd` uses the XDG desktop portal for "open a folder", so a portal (or a
`gtk3`-built `rfd`) needs to be present for that one dialog; everything else, including
dragging a folder onto the window, works without it.

Built against egui/eframe 0.35 with the wgpu backend (Metal on macOS, Vulkan on Linux).
Requires rustc ≥ 1.92. Zero bundled assets: no fonts, no icons, no images.

## Licence

Released under the [MIT licence](LICENSE-MIT).
