# Zenith — the design

How this program looks, and why. It describes what is built, not a mood board for
something adjacent: every number here has a home in the source, and the source wins if
the two ever disagree. The palettes live in `src/theme.rs`, the voices are chosen in
`src/typography.rs`, and `zenith --css` prints both as CSS so the site and the app
cannot drift apart.

The system is tonal rather than Material, discovered rather than bundled, and written
for a program with no buttons to press.

## The premise

Zenith is read-only. That is not a limitation bolted onto an editor; it is the thing the
interface is made of. There is no caret to type with, no toolbar to act from, no primary
button, no file to save. What remains is a page, a way around it, and the reader's
attention. Every decision below protects that attention.

Three consequences run through the whole system:

- **Chrome recedes.** Furniture is a slightly different value of the same paper, divided
  by hairlines. Nothing is raised unless it is a sheet laid on top of the page.
- **Contrast means structure.** Colour tells you what a token *is* in the grammar, not
  how excited to be about it.
- **The page is the product.** Sidebars, sheets and readouts exist to get you back to
  it. None of them is allowed to become the interesting thing.

## Three voices

A code editor is set in one monospace face, everywhere, forever. Zenith sets code in a
monospace voice, comments and prose in a reading voice, and chrome in a humanist sans.
Each face does the job it was drawn for.

| voice | job | what it is on this machine | sizes at 1.00× |
|---|---|---|---|
| **code** | identifiers, keywords, literals, file names, symbol names, paths | SF Mono (falling back to JetBrains Mono, Hack, Menlo…) | 15 pt body; 12 pt panels; 11 pt metrics |
| **prose** | comments, doc comments, Markdown, headings, the settings rail | Iowan Old Style (falling back to Charter, Palatino…) | 16.6 pt beside 15 pt code |
| **chrome** | labels, hints, settings, the status rail, the masthead | SF Pro (falling back to Inter, Helvetica Neue…) | 13.5 pt body; 12.5 pt controls; 11 pt hints |

**Nothing is bundled.** The machine is scanned, every candidate has to prove it can draw
source code (a real underscore, full ASCII, a parse under the same rasteriser egui uses),
and the reader can choose among the survivors. Three families are the *archetypes* — a
coding mono (JetBrains Mono), a book serif (Newsreader), a humanist sans (Work Sans) —
and all three are in the preference lists. A machine that owns them finds them in the
Type panel; a machine that does not is not a lesser zenith.

**Prose is matched by x-height, not point size.** A serif at the same nominal size as a
mono looks smaller. Comments are set at the ratio of the two faces' lowercase heights
(about 1.10× here), so the page has one optical size rather than two.

**Tracking is a material.** Section labels and the masthead are the only letterspaced
type in the program: `ZENITH` at 0.14 em, `FOLDER · src` and `CONTENTS` at 0.12 em. The
tracking is what makes a nine-pixel label read as a printed plate rather than as tiny UI.

## The tonal ladder

Every token class gets a position on one muted ladder, not a hue from a rainbow. The
ladder is low-chroma and moves mostly in lightness. Keywords are not "blue"; they are the
darkest ink. Comments are not "green"; they are the faintest. Fifteen tones, no more:

| tone | in the grammar | how it is set |
|---|---|---|
| `Keyword` | `fn`, `struct`, `if`, `class` | darkest ink |
| `KeywordSoft` | `pub`, `async`, lifetimes | the same word, spoken quietly |
| `Type` | `String`, `HashMap`, `u32` | one step cool |
| `Call` | function and method names | one step warm |
| `Constant`, `Number`, `Str` | literals | three neighbours |
| `Meta` | `#[derive]`, `@decorator`, `println!` | amber, a margin note |
| `Comment`, `Doc` | prose | faintest ink, in the serif |
| `Operator`, `Punct` | `=`, `->`, brackets, commas, `::` | nearly a whisper |
| `Raw` | code inside a prose document | mono, always |
| `Invalid` | what the parser could not read | the one honest warning |

**Punctuation is dimmed on purpose.** Brackets and semicolons are the densest marks on a
code page and carry the least meaning while reading. They are set in `Punct`, not in the
plain ink, so blocks read as shapes and identifiers come forward.

There is no `primary`, `secondary` or `error` hierarchy. A read-only program has no
actions to rank and no diagnostics to colour. `Invalid` exists for unreadable source, not
for scolding the reader.

## Six atmospheres

A theme is a place, not a hue rotation: paper, furniture, ink, accent and all fifteen
tones chosen together. Three are daylight, three are lamplight.

| theme | light | the material it is made of |
|---|---|---|
| **Lugano** | day | pale stone, lake blue, cypress, terracotta roofs |
| **Maionese** | day | butter paper, yolk gold, gherkin, white pepper |
| **Bibione** | day | sand, pine shade, the Adriatic, a striped umbrella |
| **Kununurra** | night | pindan earth at dusk, sandstone, spinifex, the Ord's turquoise |
| **Panittha** | night | teak, gold leaf, jade, saffron, a little lacquer |
| **Newsprint** | day | cold stock, sharp eyes, no atmosphere at all |

Lugano is the default. There is no "dark mode": there are lamplit themes, which are
designed rather than inverted, and the window tells the system which of the two it is so
that its own materials agree.

## Surfaces and edges

A palette has twelve environmental roles. They are the vocabulary; nothing outside
`src/theme.rs` invents a colour.

| role | what it is |
|---|---|
| `paper` | the reading page, and the ground behind every other surface |
| `panel` | the desk: top bar, sidebars, status rail, sheets |
| `panel_deep` | recessed furniture: fields, wells |
| `edge` | every hairline in the program |
| `ink` | primary text, the active row, the file you are reading |
| `ink_soft` | secondary text, inactive rows |
| `ink_faint` | labels, hints, marks, the names of things not chosen |
| `accent` | the cursor, the spine, links, the filled part of a slider |
| `accent_soft` | hover, glow, the quiet half of a link |
| `selection` | a chosen row's wash |
| `guide` | the indentation hairlines |
| `fade` | the focus lens' scrim |

**Elevation is paper, not light.** A surface is higher because it is a lighter sheet with
a hairline and a diffused shadow, not because it glows. There are three levels:

1. **Desk** — top bar, folder panel, contents, status rail. `panel`, no radius, no shadow.
2. **Page** — the central reading canvas. `paper`, square, edge to edge between the panes.
3. **Sheet** — settings, finders, the key list. `panel`, 14 pt corners, a 1 pt `edge`
   stroke, and one soft shadow.

The page never floats. The panes never round. A sheet is the only *elevated* thing, and
the only generous corner in the program, which is what makes it read as something you
put down on the desk.

## Layout

A tripartite desk at native window scale (1440 × 920 by default):

| region | default | range | material |
|---|---|---|---|
| masthead | 40 pt (+ the system's title strip on macOS) | fixed | `panel` |
| folder panel | 252 pt | 200–340, resizable | `panel` |
| reading page | fluid | the rest | `paper` |
| contents | 240 pt | 180–340, resizable | `panel` |
| status rail | 24 pt | fixed | `panel` |

Panes are separated by **one continuous hairline** in `edge`, not by a gap, a shadow or a
second background. The same line runs under the masthead and above the status rail, so
the frame is drawn rather than implied.

Every region can fold away. `tab` moves the keyboard to the folder, `o` toggles the
contents, `Z` removes all of it: in zen the page is the window, the ribbon beside the
cursor is gone, and only the page and its margin marks remain.

## The page

The page is set to a **measure**, not to the window. It is the single most consequential
number in the program, and it is the reader's:

| control | default | range | why |
|---|---|---|---|
| measure | 86 ch | 60–100 | beyond 100 a line is a corridor, not a sentence |
| leading | 1.62 | 1.30–1.90 | air below the line, not between the words |
| type size | 1.00× | 0.80–1.80 | legibility outranks the grid |

A design system that stops at 1.40× is a design system for people with good eyes. This
one goes higher, and every value between the bounds keeps the same measure and the same
columns.

On the page:

- **Indentation is drawn, not typed.** Leading whitespace is removed from the text and
  painted as one fading hairline per level in `guide`. A block's shape is felt, not
  counted.
- **Comments are prose.** The `//` or `#` stays behind in faint mono, like a printer's
  pilcrow; the sentence after it is serif.
- **Long lines wrap and admit it.** Every continuation row carries `↳` in the margin, so
  a wrapped expression is never mistaken for a new one.
- **Blank lines are compressed** to 46% of a row. Paragraph rhythm survives; the page does
  not become a corridor.
- **The cursor is a reading position.** A 2 pt ribbon in the margin, a 14-alpha wash over
  the row, and a hairline at the column. It is not a caret and it does not blink.
- **The spine is a bookmark, not a scrollbar.** A 2 pt mark in `accent` lives at the edge
  of the sheet, shows how far through the file you are, and can be dragged. It is the only
  scroll indicator; egui's floating bar is hidden.
- **The focus lens is a gradient of attention**, painted as a scrim in `fade` over
  everything outside the block you are in. The page comes forward; the text does not
  change colour.

Headings in a rendered Markdown document are the prose face, larger, in the keyword ink,
with air above and none below. A code fence or a quotation gets one flat band of 10% ink
and a 3 pt corner; nothing gets a border, a shadow or a second background colour.

## Shape

| thing | radius | note |
|---|---|---|
| page, panes, band rules | 0 | a pane is a rectangle |
| cursor wash, row wash | 2–4 pt | soft enough not to read as a button |
| toggles, fields, small controls | 2–4 pt | tactile, not pill-shaped |
| theme swatch | 8 pt | a card, not a sheet |
| sheet (settings, finders, keys) | 14 pt | the only generous corner |

Fourteen, not twenty-four. A 24 pt corner on a 560 pt sheet reads as a phone app placed on
a desk; 14 pt reads as a sheet lifted at one corner. This is furniture, not a device.

The sheet shadow is `0 18 48` in the page's ink at 18% (light) or 43% (dark), with a
hairline the whole way round. Nothing inside the page casts a shadow.

## Components

### Masthead

`ZENITH` in chrome, 10.5 pt, uppercase, tracked 0.14 em, in `ink_faint` — an imprint, not
a logo. Then `›`, then the path: directories in chrome 12 pt `ink_faint`, the open file in
the **code face**, 12.5 pt, `ink`. The path is the one place in the chrome where a name
from the codebase is shown, so it is set in the voice of the codebase.

### Status rail

24 pt, `panel`, hairline above. On the left, the facts: language, position, line count,
and notices, set in the **code face** at 11 pt because they are measurements. On the
right, the one standing invitation, in chrome 11 pt `ink_faint`:

`⌘P find file · Z zen · ? keys`

When the folder is focused the hint changes to its keys; when there is news, the news
takes the whole strip and the invitation steps aside for seven seconds. The rail never
shows two messages at once.

### Sidebars

Both are index cards for the code, so both are set in the **code face** at 12 pt: file
names and symbol names are names, not prose. Titles — `FOLDER · src`, `CONTENTS` — are
chrome, 10 pt, uppercase, tracked 0.12 em, `ink_faint`.

Rows are 22 pt, indented 12 pt per level, selected with a `selection` wash and a 4 pt
corner, and marked with a 2 pt accent tab in the margin. A name too long for the panel
is cut with an ellipsis and the full name is shown on hover. Carets are drawn as
triangles rather than typed, because `▸` and `▾` are missing from fonts on some
machines. Contents marks are typographic: `ƒ` function, `○` type or constant, `+` module.

### Settings sheet

560 pt wide, `panel`, 14 pt corners, 1 pt `edge`, the sheet shadow, and a backdrop of the
page's ink at 17%. A 20 pt serif title, a hairline, then a 118 pt rail of sections —
`Page`, `Type`, `Theme`, `Language` — in the prose voice at 14.5 pt, active in `ink`,
inactive in `ink_faint`. A hairline divides the rail from the page of the sheet.

Every control is live. There is no Apply: taste should be visible while it is being
changed. `restore defaults` sits in a footer under its own hairline, alone, in
`ink_faint`; the first click turns it into `yes, reset` in the fault tone, and the second
one acts. It is the one action here that cannot be undone by putting it back by hand.

### Theme swatches

Two columns, 175 × 84 pt each (the sheet decides the number), 8 pt corners, the theme's
own paper inside. Each card is a working page: the
theme's name in its serif, then four lines of the same sample program, syntax and all.
A swatch is never tinted to show that it is chosen; it is outlined 1.5 pt in its own
`accent`. Names and colours alone cannot tell you how a page reads; a paragraph can.

### Toggles

Not egui's checkbox. A reader's toggles are drawn as an 11 pt hairline square in a
70-alpha ink when off, and the same square with an ink check turned inside it when on.
Off is not a filled pill and never a circle: a circle means "choose one", and these are
independent statements.

### Sliders

A label in chrome 12.5 pt and a value in the code face 11.5 pt on one line, then a 1 pt
`edge` rule with the chosen part in `accent` and a 1.5 pt pip as the hand. Thirty points
of vertical room, no knob, no track pill. The value is right-aligned and monospaced so
that dragging a number does not make the labels jump.

### Finders and the key list

`⌘P` and `/` are sheets too: `panel`, hairlines, the same shadow, file paths in the code
face. They appear where they are useful — the file finder at the top, the in-file search
at the bottom edge — and they are the only things in the program that take typed text.

The key list (`?`) is a sheet with the prose title "Reading keys", keys in `accent` mono
and descriptions in chrome.

## Motion

There is none. Nothing fades, slides, springs or eases. The page moves when the reader
moves it, and a landing is instant. The one soft thing in the program is a shadow, and it
is soft because paper is.

## What this system refuses

- **A Material colour hierarchy.** No primary/secondary/tertiary, no containers, no
  error surfaces. Those are the tokens of a program with actions and failures. This one
  has neither.
- **Bundled type.** The page must look like the reader's machine, and no font licence
  should be riding along inside a reader.
- **A fixed scheme.** Six atmospheres, each designed as a place. A single "light theme"
  with a "dark variant" would be a colour scheme, which is what this program exists not
  to be.
- **Diagnostics as decoration.** No red squiggles, no error counts, no badges. A broken
  thing gets one line in the status rail.
- **Icons.** Every mark is drawn (carets, the cursor ribbon) or typographic (`ƒ`, `○`,
  `+`, `↳`). No icon set, no emoji.
- **Shadows inside the page.** Elevation belongs to sheets on the desk. Text is flat.
- **The editor's furniture.** No minimap, no gutter of line numbers by default, no
  breadcrumbs of symbols, no tabs, no toolbar, no toolbar's worth of menus.
- **Writing into what is read.** Configuration, reading positions and bookmarks live
  outside the repository, always.
