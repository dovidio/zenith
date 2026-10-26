# Zenith — editorial code

The design system behind the reading room. It is a specification: the palettes, the
voices and the reading page are as built; the interaction model below is the target.
Where the source and this document disagree, this document is the intention and the source
is what exists today.

## The premise

Zenith is read-only. That is not a limitation bolted onto an editor; it is the thing the
interface is made of. There is no caret to type with, no toolbar to act from, no primary
action, no file to save. What remains is a page and the reader's attention.

The program opens as a page. Chrome is not where you live; it is something you summon and
dismiss. The model is a writing surface, not a workspace. A reader should be able to spend
an hour with no permanent control on screen.

Three consequences run through the system:

- **Chrome is summoned.** Nothing is pinned. A drawer or a sheet is open because it was
  asked for, and it goes away when it is done.
- **Contrast means structure.** Colour tells you what a token *is* in the grammar, not
  how excited to be about it.
- **The page is the product.** The index, the contents, the launcher and settings exist
  to get you back to it. None of them is allowed to become the interesting thing.

The bloom is the one piece of chrome that offers itself, at startup and when the pointer
reaches for it. It is how the doors are seen; everything else waits to be asked.

## Three voices

A code editor is set in one monospace face, everywhere, forever. Zenith sets code in a
monospace voice, comments and prose in a reading voice, and chrome in a humanist sans.
Each face does the job it was drawn for.

| voice | job | what it is on this machine | sizes at 1.00× |
|---|---|---|---|
| **code** | identifiers, keywords, literals, file names, symbol names, paths | SF Mono (falling back to JetBrains Mono, Hack, Menlo…) | 15 pt body; 12 pt drawers; 11 pt metrics |
| **prose** | comments, doc comments, Markdown, headings, the settings rail | Iowan Old Style (falling back to Charter, Palatino…) | 16.6 pt beside 15 pt code |
| **chrome** | labels, plates, settings, the launcher, notices | SF Pro (falling back to Inter, Helvetica Neue…) | 13.5 pt body; 12.5 pt controls; 11 pt notices |

**Nothing is bundled.** The machine is scanned, every candidate has to prove it can draw
source code (a real underscore, full ASCII, a parse under the same rasteriser egui uses),
and the reader can choose among the survivors. Three families are the *archetypes* — a
coding mono (JetBrains Mono), a book serif (Newsreader), a humanist sans (Work Sans) —
and all three are in the preference lists. A machine that owns them finds them in the
Type panel; a machine that does not is not a lesser zenith.

**Prose is matched by x-height, not point size.** A serif at the same nominal size as a
mono looks smaller. Comments are set at the ratio of the two faces' lowercase heights
(about 1.10× here), so the page has one optical size rather than two.

**Tracking is a material.** Section labels are the only letterspaced type in the program:
`FOLDER · src` and `CONTENTS` at 0.12 em. The tracking is what makes a nine-pixel label
read as a printed plate rather than as tiny UI.

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
| `panel` | the index, the contents, and every sheet |
| `panel_deep` | recessed furniture: fields, wells |
| `edge` | every hairline in the program |
| `ink` | primary text, the active row, the file you are reading |
| `ink_soft` | secondary text, inactive rows |
| `ink_faint` | plates, marks, the names of things not chosen |
| `accent` | the cursor, the spine, links, the filled part of a slider |
| `accent_soft` | hover, glow, the quiet half of a link |
| `selection` | a chosen row's wash |
| `guide` | the indentation hairlines |
| `fade` | the focus lens' scrim |

**Elevation is paper, not light.** There are three levels:

1. **Page** — the reading canvas. `paper`, square, the whole window.
2. **Drawer** — the index and the contents. `panel`, square, attached to the page by one
   hairline. It is not floating, and it casts no shadow.
3. **Sheet** — the launcher, settings and search. `panel`, 14 pt corners, a 1 pt `edge`
   stroke, one soft shadow, and the page dimmed by 17% of its own ink.

The page is the ground. A drawer is attached to it; a sheet is laid over it. Nothing else
is elevated, and nothing inside the page casts a shadow.

## Layout

The window has one permanent surface: the page. Everything else is a state.

| state | on screen |
|---|---|
| **reading** (default) | the page, with the bloom at the top |
| **index** | the page and the left drawer |
| **contents** | the page and the right drawer |
| **launcher** | the page and the launcher sheet |
| **settings** | the page and the settings sheet |
| **search** | the page and the search sheet at the foot |

The index is 252 pt wide and resizes 200–340. The contents are 240 pt wide and resize
180–340. A drawer meets the page at one continuous hairline in `edge`; there is no gap and
no second background. The page re-centers in the remaining width; a drawer takes room from
the margins before it takes room from the measure.

**One summoned surface at a time.** Opening a sheet puts a drawer away; opening a drawer
closes the launcher or settings. The launcher itself replaces a drawer before it appears.
There is no stack of panels to get lost in.

The window's title — where the desktop draws one — is the only chrome that never leaves,
and it names the file. Everything the program draws is summoned.

There is no status bar. The foot of the window is not furniture.

## The page

A file names itself at the head of its page: the name, the path under it in the reading
face, and a hairline. That title block is part of the page, not chrome; it scrolls away
with the text. There is no masthead above the page.

The page keeps 96 pt of air above the title block. That is the bloom's room, and the pause
before the title.

When there is nothing to read at all, the page is not silent: it says `Open a folder…` in
one line, centered, as a row. It is a row like any other — a wash on hover, never a hand.
That is the only control the page itself carries, and it leaves as soon as there is a file.
Silence in an empty room is not calm; it is a dead end.

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
| page, drawers, band rules | 0 | a pane is a rectangle |
| cursor wash, row wash | 2–4 pt | soft enough not to read as a button |
| toggles, fields, buttons, small controls | 2–4 pt | tactile, not pill-shaped |
| theme swatch | 8 pt | a card, not a sheet |
| bloom bubble | full | the only ring; it is a bubble, not a button |
| sheet (launcher, settings, search) | 14 pt | the only generous rounded rectangle |

Fourteen, not twenty-four. A 24 pt corner on a 560 pt sheet reads as a phone app placed on
a desk; 14 pt reads as a sheet lifted at one corner. This is furniture, not a device.

The sheet shadow is `0 18 48` in the page's ink at 18% (light) or 43% (dark), with a
hairline the whole way round.

## Components

### The bloom

The one piece of chrome that offers itself. At the top of the window's content, centered,
below the desktop's title strip where there is one, 14 pt down, is a 7 pt dot in
`ink_soft` inside a 28 pt hit area. On hover a 20 pt hairline ring appears around it and the
dot warms. It is the seed, not a menu.

At startup the bloom is open: the dot and four 26 pt bubbles in a shallow fan below it,
each with a short hairline stem back to the dot and a name in 9.5 pt chrome, tracked
0.08 em, beneath it — `INDEX`, `CONTENTS`, `LAUNCHER`, `SETTINGS`. The bubbles are
paper-filled circles with a 1 pt `edge` ring; the marks inside are drawn, not typed: a trunk
with two branches, three rules, a ring with a handle, two slider rules. The rings are
`edge`, the marks `ink_soft`, the labels `ink_faint`; on hover the ring takes `accent`, the
mark and label take `ink`, and the stem warms. A bubble and its label are one hit area. If
the reader does nothing for four seconds, the bubbles collapse into the dot and the whole
bloom dissolves.

After that it is summoned by approach. The pointer entering the top 56 pt of the window
within 240 pt of its center brings the dot back; it is a hover zone and never steals a
click from the page. Clicking the dot buds the four bubbles; clicking it again, or `esc`,
collapses them. Resting on the dot for 200 ms also buds them, so the icons can be found
without a click. Clicking a bubble opens its surface and the bloom goes away; a click
anywhere else collapses it, and on the page that click also places the reading cursor.
Once summoned, the bloom collapses to the dot after three and a half seconds without a
hover or a click, and the dot fades two and a half seconds after the pointer leaves the top
zone; it does not disappear while the pointer is on it. Nothing in the bloom gets a hand;
hover is a ring and a change of ink.

The bloom is fixed to the window, not to the page. At the top of a file it sits in the
96 pt of air above the title block; when the page is scrolled it floats over the text,
which is why it is small, paper-filled, and never stays. The names belong to the margin:
when the bloom floats over scrolled text, the labels are left out and the marks alone are
enough. It is hidden while a drawer or a sheet is open.

Everything else is summoned; this one has to offer itself, or the doors would be invisible.

### The index

A left drawer, 252 pt, resizable, `panel`, one hairline on its right. The plate
`FOLDER · name` at its head, chrome 10 pt, tracked 0.12 em, `ink_faint`.

The rows are set in the code face at 12 pt, 22 pt high, indented 12 pt per level; a
selected row gets a `selection` wash, a 4 pt corner and a 2 pt accent tab. Carets are
drawn triangles, because `▸` and `▾` are missing from some machines. A name too long for
the drawer is cut with an ellipsis and shown in full on hover.

The tree is a map, not a finder: finding a file is the launcher's job. Choosing a file
reads it and leaves the index open, so the reader can walk the tree. `esc`, or a click on
the page, leaves it. Its bubble is the tree mark in the bloom; a keyboard reader reaches
the same drawer with the launcher's `Index`.

### The contents

A right drawer, 240 pt, resizable, the same plate and the same rhythm, headed `CONTENTS`.
Symbol names are set in the code face; the marks are typographic — `ƒ` function, `○` type
or constant, `+` module. The symbol the cursor is inside is washed and marked with the
same 2 pt tab as the index. Clicking a symbol moves the page and leaves the drawer open;
`esc`, or a click on the page, closes it.

It is summoned from its bubble in the bloom, or from the launcher. It is a view, not a
permanent sidebar.

### The launcher

The room's search, and one of the bloom's four bubbles. A sheet 560 pt wide, one third of
the way down the window at 1.00×, opened by its bubble or with `⌘K` (or `ctrl-K`). It
behaves like a spotlight: one field, a few results, no menus. Its input is chrome 16 pt,
with no box: a hairline under the line is the only boundary. The placeholder is
`a file, a view, or an action`.

The results are a short, ranked list, not a manual: file names in the code face, views and
actions in chrome, 28 pt rows, selected with a wash and a 2 pt accent tab. There is no hand
cursor. It never shows more than eight rows; a launcher that becomes a menu is a menu, and
the answer to a longer list is a more specific query. At 1.30× and above the sheet moves
up to the top air, and its body can scroll, so eight rows at 1.80× are still eight rows,
not a sheet clipped by the window.

The empty state is contextual, in three groups. `HERE` comes first — `Contents`, then
`Search this file`. `FOLDER` follows — `Index`, the matching files, and `Open folder…`
when there is no file open or no folder yet. `ZENITH` carries the page's own switches and doors — `Focus lens`, `Soft wrap`, `Line
numbers`, `Distilled`, `Render markdown`, `Next theme` / `Previous theme`, `Larger type` /
`Smaller type`, `Settings` and `Keys`. With no file open the `HERE` group is absent; with
no folder, `Open folder…` is the only folder action. The list is curated to eight; the rest
is one query away.

A query flattens the groups and ranks the results. Commands beat files on a tie; a file is
still a first-class result. Typing `theme` surfaces the two theme steps before the source
file named `theme.rs`; typing `line` surfaces `Line numbers`, not `Contents` because its
keywords include `outline`; typing `app` is a file search again. The launcher is where the
reading state advertises what else the program can do; the empty page has one door of its
own.

An action with a key carries it as a **keycap** — the real shortcut, in code 10.5 pt, in a
`panel_deep` recess with a 1 pt `edge` hairline and a 3 pt corner. It is a key, not a
badge, and it is the one place the page advertises a shortcut: the reader can use the
launcher once, learn the key, and take the direct route the next time. Actions that toggle
something also carry their current state as a chrome word immediately after the name —
`Focus lens off`, `Soft wrap on` — so the list is a set of switches, not a set of
instructions. `Soft wrap` has no key and therefore no keycap; it is honest about that
rather than inventing one.

It launches places, views and the page's switches, not motions. `j`, `k`, `gd` and the rest
are the page's grammar, and they never appear as commands here. The bloom shows the four
doors; the launcher is the one for anything that is not a place in the index or the
contents.

`⌘P` opens the same sheet scoped to files; `s` and `⌘,` open settings directly. These are
back doors for people who already know them. The complete key map is in Settings › Keys;
`?` opens it.

`esc` or a click outside closes it; the arrows move; enter or a click opens.

### Notices

There is no status bar and no footer. A sentence the program has to say appears at the
foot of the page, in chrome 11 pt `ink_soft`, 16 pt above the bottom edge, with no bar and
no background. It leaves after seven seconds. The rest of the time the foot is empty.

The language, the position and the line count are not permanent furniture. The title block
names the file, the spine shows how far you are, and the page shows the rest.

### Settings

Summoned by its bubble in the bloom, by the launcher's `Settings`, or with `s` / `⌘,`.
A centered sheet 560 pt wide, `panel`, 14 pt corners, 1 pt `edge`, and the sheet shadow.
A 20 pt serif title, a hairline, then a 118 pt rail of five sections — `Page`, `Type`,
`Theme`, `Language`, `Keys` — in the prose voice at 14.5 pt, active in `ink`, inactive in
`ink_faint`. A hairline divides the rail from the page of the
sheet and runs all the way to the footer.

`Keys` is the complete reference: grouped by reading, files, views and settings, scrollable
inside the sheet. It is a page in settings, not an overlay, and it is allowed to be long
without covering the page. The old `?` key opens this page; it no longer opens a sheet of
its own.

Every control is live. There is no Apply: taste should be visible while it is being
changed.

`restore defaults` is a **button**, not a text link. It is 26 pt high, with a 1 pt `edge`
border, a 4 pt corner, chrome 11.5 pt in `ink_soft`, and no fill. On hover the fill is 6%
ink and the border strengthens; on press it is 10%. The first click turns it into
`restore defaults?` in the palette's fault tone with a fault-coloured border; a second
click within five seconds acts. It is the only button on the sheet: everything else that
can be clicked is a row, a tab, a toggle, a swatch or a slider.

**No hand cursor in settings.** The tabs, the rows, the swatches, the sliders and the
button all keep the arrow. Only text fields get the I-beam. Clickability is shown by ink,
fill and border, not by a cursor change. `esc` or a click on the backdrop closes the sheet.

### Theme swatches

Two columns, 175 × 84 pt each (the sheet decides the number), 8 pt corners, the theme's
own paper inside. Each card is a working page: the theme's name in its serif, then four
lines of the same sample program, syntax and all. A swatch is never tinted to show that it
is chosen; it is outlined 1.5 pt in its own `accent`. Names and colours alone cannot tell
you how a page reads; a paragraph can. The card keeps the arrow on hover; its outline
answers instead.

### Toggles

Not egui's checkbox. A reader's toggles are drawn as an 11 pt hairline square in a
70-alpha ink when off, and the same square with an ink check turned inside it when on. Off
is not a filled pill and never a circle: a circle means "choose one", and these are
independent statements. The row keeps the arrow; hover is a wash, not a cursor.

### Sliders

A label in chrome 12.5 pt and a value in the code face 11.5 pt on one line, then a 1 pt
`edge` rule with the chosen part in `accent` and a 1.5 pt pip as the hand. Thirty points of
vertical room, no knob, no track pill. The value is right-aligned and monospaced so that
dragging a number does not make the labels jump. The arrow stays the arrow; the pip turns
accent on hover.

### Search

`/` or `⌘F` opens the in-file search at the foot of the page: a sheet, 14 pt corners, the
field at the left, the match count at the right. It searches the text of the file you are
reading; it is not the launcher. Enter and shift-enter step through matches; `esc` closes.

Finding a *file* is the launcher's job. There is no second file finder.

## Motion

The page is still. It never fades, slides, springs or eases; a landing is instant; a drawer
or a sheet arrives without transition. The bloom is the one exception, because a bubble
has to come from somewhere.

Opening: the dot's ring divides into four stems, and each bubble is a duplicate of that
ring — born at the dot at 0.2 scale and zero opacity, travelling along its stem, and
resolving its own mark as it settles — 200 ms, staggered 30 ms from the centre out.
Closing: the bubbles return and fade in 140 ms with no stagger. The dot fades in over
160 ms when the pointer reaches the top, and out over 600 ms when it is left. Idle: the
bubbles collapse to the dot, and then the dot fades.

That is the whole vocabulary: one bud, one dissolve, at the top of the window. The one soft
thing is a sheet's shadow; the one moving thing is the bloom.

## What this system refuses

- **Chrome by default.** No permanent masthead, no pinned sidebar, no status bar, no
  toolbar. Zen is not a mode to enter; it is the state you start in.
- **A shortcut legend.** No hints at the foot, no `?` overlay. A command may carry its own
  key in the launcher; the complete list is in Settings › Keys.
- **A status bar.** Notices are one line at the foot of the page and then they are gone.
- **A hand cursor.** The arrow is the native pointer; the I-beam is for fields. The hand
  belongs to links on the web.
- **A pinned panel.** At most one drawer or sheet at a time, and only when asked for.
- **Animation everywhere.** The page, the drawers and the sheets arrive and go without a
  transition. The bloom buds and dissolves; nothing else moves.
- **A Material colour hierarchy.** No primary/secondary/tertiary, no containers, no error
  surfaces. Those are the tokens of a program with actions and failures. This one has
  neither.
- **Bundled type.** The page must look like the reader's machine, and no font licence
  should be riding along inside a reader.
- **A fixed scheme.** Six atmospheres, each designed as a place. A single "light theme"
  with a "dark variant" would be a colour scheme, which is what this program exists not
  to be.
- **Diagnostics as decoration.** No red squiggles, no error counts, no badges. A broken
  thing gets one line at the foot of the page.
- **Icons.** Every mark is drawn (the bloom's bubbles, carets, the cursor ribbon) or
  typographic (`ƒ`, `○`, `+`, `↳`). No icon set, no emoji.
- **Shadows inside the page.** Elevation belongs to the sheets laid over the page. Text
  is flat.
- **The editor's furniture.** No minimap, no gutter of line numbers by default, no
  breadcrumbs of symbols, no tabs, no toolbar, no toolbar's worth of menus.
- **Writing into what is read.** Configuration, reading positions and bookmarks live
  outside the repository, always.
