//! Zenith — a reading room for source code.
//!
//! Usage:
//!   zenith [FOLDER|FILE]        open a repository (or a file inside one) and read
//!   zenith --fonts              list the typefaces zenith can find and use
//!   zenith --dump FILE          parse FILE headlessly and print its tones (no window)
//!   zenith --smoke OUT.png      render a few frames, save a self-portrait, exit
//!
//! Zenith never writes to anything it reads.

mod app;
mod catalog;
mod distill;
mod fuzzy;
mod markdown;
mod outline;
mod png;
mod reader;
mod folder;
mod syntax;
mod theme;
mod typography;
mod vim;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode = Mode::Run;
    let mut target: Option<PathBuf> = None;
    let mut theme_override: Option<String> = None;
    let mut measure_override: Option<usize> = None;
    let mut scale_override: Option<f32> = None;
    let mut mode_flag_distilled: Option<bool> = None;

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--fonts" => mode = Mode::Fonts,
            "--dump" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Dump(PathBuf::from(p)),
                    None => die("--dump needs a file path"),
                }
            }
            "--grammars" => mode = Mode::Grammars,
            "--faces" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Faces(PathBuf::from(p)),
                    None => die("--faces needs a font file"),
                }
            }
            "--scopes" => {
                i += 1;
                let p = args.get(i).map(PathBuf::from).unwrap_or_else(|| die("--scopes needs a file"));
                let n = args
                    .get(i + 1)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(6);
                mode = Mode::Scopes(p, n);
            }
            "--smoke" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Smoke(PathBuf::from(p)),
                    None => die("--smoke needs an output path"),
                }
            }
            "--css" => mode = Mode::Css,
            "--glyphs" => mode = Mode::Glyphs,
            "--md" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Md(PathBuf::from(p)),
                    None => die("--md needs a file path"),
                }
            }
            "--walk" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Walk(PathBuf::from(p)),
                    None => die("--walk needs a folder"),
                }
            }
            "--distill" => {
                i += 1;
                match args.get(i) {
                    Some(p) => mode = Mode::Distill(PathBuf::from(p)),
                    None => die("--distill needs a file path"),
                }
            }
            "--distilled" => mode_flag_distilled = Some(true),
            "--no-distilled" => mode_flag_distilled = Some(false),
            "--theme" => {
                i += 1;
                theme_override = args.get(i).cloned();
            }
            "--measure" => {
                i += 1;
                measure_override = args.get(i).and_then(|v| v.parse().ok());
            }
            "--scale" => {
                i += 1;
                scale_override = args.get(i).and_then(|v| v.parse().ok());
            }
            "-h" | "--help" => {
                println!("{}", HELP);
                return;
            }
            other if other.starts_with('-') => die(&format!("unknown flag {other}")),
            other => target = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    match mode {
        Mode::Fonts => typography::print_available(),
        Mode::Dump(path) => dump(&path),
        Mode::Grammars => grammars(),
        Mode::Faces(path) => typography::print_faces(&path),
        Mode::Distill(path) => distill(&path),
        Mode::Glyphs => print_glyphs(),
        Mode::Css => print!("{}", theme::css()),
        Mode::Md(path) => print_md(&path),
        Mode::Walk(path) => walk(&path),
        Mode::Scopes(path, n) => dump_scopes(&path, n),
        Mode::Smoke(out) => app::run_smoke(target, out, theme_override, measure_override, scale_override),
        Mode::Run => app::run(
            target,
            theme_override,
            measure_override,
            scale_override,
            mode_flag_distilled,
        ),
    }
}

enum Mode {
    Run,
    Fonts,
    Dump(PathBuf),
    Grammars,
    Faces(PathBuf),
    Distill(PathBuf),
    Glyphs,
    Css,
    Md(PathBuf),
    Walk(PathBuf),
    Scopes(PathBuf, usize),
    Smoke(PathBuf),
}

const HELP: &str = "\
zenith — a reading room for source code

USAGE:
    zenith [FOLDER|FILE]      open a repository (or a file) and read
    zenith --fonts            list available typefaces
    zenith --dump FILE        print parsed tones for a file, headlessly
    zenith --grammars         list the grammars zenith can set text in
    zenith --css              print the themes as CSS, for a site in the same ink
    zenith --distill FILE     show which tokens --distilled would recede, per file
    zenith --distilled        fade types and punctuation (strongly typed languages)
    zenith --faces FILE       list the faces inside a font file, and their coverage
    zenith --scopes FILE [N]  show the grammar scopes behind each tone
    zenith --smoke OUT.png    render a self-portrait and exit

READING KEYS:
    j / k, space, ⇧space      move the reading cursor / page
    n / p                     next / previous file in the folder
    enter                     jump to the outline entry under the cursor line
    /  or ⌘K                  find a file in the folder
    o                         outline panel
    y                         zen mode (hide all chrome)
    f                         focus lens — dim everything but the block you read
    t                         next theme
    T                         previous theme
    m                         change the code voice (mono typeface)
    M                         change the prose voice (serif typeface)
    [ / ]                     smaller / larger type
    w                         toggle soft wrap
    #                         toggle line numbers
    g / G                     top / bottom of file
    b                         bookmark this file
    q                         close the file
    ⌘O                        open a folder
    ⌘,                        settings panel
    ?                         help
";

fn die(msg: &str) -> ! {
    eprintln!("zenith: {msg}");
    eprintln!("try `zenith --help`");
    std::process::exit(2);
}

/// Show how a Markdown file is rendered: what each line becomes, markers and all.
fn print_md(path: &std::path::Path) {
    let hl = syntax::Highlighter::new();
    let bytes = std::fs::read(path).unwrap_or_default();
    let n = bytes.len().min(4096);
    let head = String::from_utf8_lossy(&bytes[..n]).lines().next().unwrap_or("").to_string();
    let syntax = hl.syntax_for(path, &head).clone();
    let text = match syntax::read_readable(path) {
        Ok(syntax::Readable::Text { text, .. }) => text,
        _ => {
            eprintln!("zenith: {} is not text", path.display());
            return;
        }
    };
    let doc = hl.parse(&syntax, &text, text.len());
    let md = markdown::render(&doc.lines);
    for (i, line) in md.iter().enumerate() {
        println!("{:>4}  {:<14} {}", i + 1, format!("{:?}", line.kind), line.text());
    }
}

/// Walk a folder the way the reader does, and say what it cost.
///
/// Reading a large tree is the slowest thing zenith does, and the reasons are not
/// obvious: a stat call per file, plus the repository's own ignore rules. This is the
/// measurement, so "it is slow" becomes a number and a cause.
fn walk(root: &std::path::Path) {
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let t = std::time::Instant::now();
    match folder::open(root, &name, true, false) {
        Ok(s) => {
            println!("{}\n", root.display());
            println!("  files              {}", s.files.len());
            println!("  filtered           {}", s.filtered);
            println!("  unreadable         {}", s.unreadable);
            println!("  stopped at the cap {}", s.truncated);
            println!("  took               {:.2?}", t.elapsed());
            if s.truncated {
                println!("\n  the folder stops at 40,000 files; this tree is larger");
            }
        }
        Err(e) => eprintln!("zenith: cannot read {}: {e}", root.display()),
    }
}

/// Which of the interface's marks this machine can actually draw.
///
/// The interface draws triangles, diamonds and arrows as text, and system fonts disagree
/// about them: Apple's have all of these, a bare Linux install may have none. This is how
/// to tell which ones degraded, and to what.
fn print_glyphs() {
    let typo = typography::build(0, 0, 0);
    let marks = typography::Marks::of(&typo.defs, &typo.ui);
    println!("marks (what the interface draws as icons):\n");
    for (what, fallback, missing) in marks.describe() {
        println!(
            "  {what:<14} {}",
            if missing {
                format!("missing — using {fallback:?}")
            } else {
                "drawable".to_string()
            }
        );
    }
    println!(
        "\nmodifier label: {:?}   shift: {:?}",
        typography::mod_label(),
        typography::shift_label()
    );
    if typo.bundled_fallback {
        println!("note: no system fonts were found; egui's bundled faces are in use");
    }
}

/// Show exactly what distilled mode would recede, so the claim is checkable rather
/// than asserted. Metadata runs are printed in [brackets].
fn distill(path: &std::path::Path) {
    let hl = syntax::Highlighter::new();
    let head = std::fs::read(path)
        .ok()
        .map(|b| {
            let n = b.len().min(4096);
            String::from_utf8_lossy(&b[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let syntax = hl.syntax_for(path, &head);
    let text = match syntax::read_readable(path) {
        Ok(syntax::Readable::Text { text, .. }) => text,
        Ok(_) => {
            println!("binary file");
            return;
        }
        Err(e) => {
            eprintln!("zenith: {e}");
            std::process::exit(1);
        }
    };
    let doc = hl.parse(syntax, &text, text.len());
    let lang = distill::lang_of(path, &doc.syntax);
    println!(
        "{} — grammar {}, distillation: {lang:?}",
        path.display(),
        doc.syntax
    );
    let mut faded = 0usize;
    for (i, line) in doc.lines.iter().enumerate() {
        let marks = distill::marks(lang, line);
        let mut out = String::new();
        let mut hit = 0usize;
        for (t, m) in line.tokens.iter().zip(marks) {
            match m {
                Some(distill::Mark::All) => {
                    out.push('[');
                    out.push_str(&t.text);
                    out.push(']');
                    hit += 1;
                }
                Some(distill::Mark::Part(s, e)) => {
                    let chars: Vec<char> = t.text.chars().collect();
                    let (s, e) = (s.min(chars.len()), e.min(chars.len()));
                    out.extend(&chars[..s]);
                    out.push('[');
                    out.extend(&chars[s..e]);
                    out.push(']');
                    out.extend(&chars[e..]);
                    hit += 1;
                }
                None => out.push_str(&t.text),
            }
        }
        if hit > 0 {
            faded += 1;
            println!("  {:>5}  {}", i + 1, out.trim_end());
        }
    }
    println!("\n{faded} line(s) would be distilled");
}

/// List every grammar in the syntax set, with its extensions.
fn grammars() {
    let hl = syntax::Highlighter::new();
    let list = hl.grammars();
    println!("zenith can set {} grammars\n", list.len());
    for (name, exts) in &list {
        if exts.is_empty() {
            println!("  {name}");
        } else {
            println!("  {name:<28} .{}", exts.join("  ."));
        }
    }
}

/// Show the grammar's own scope stacks next to the tone zenith assigns them. This is
/// the tool that keeps the tone table honest.
fn dump_scopes(path: &std::path::Path, max_lines: usize) {
    let hl = syntax::Highlighter::new();
    let head = std::fs::read(path)
        .ok()
        .map(|b| {
            let n = b.len().min(4096);
            String::from_utf8_lossy(&b[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let syntax = hl.syntax_for(path, &head);
    let text = match syntax::read_readable(path) {
        Ok(syntax::Readable::Text { text, .. }) => text,
        Ok(_) => {
            println!("binary file");
            return;
        }
        Err(e) => {
            eprintln!("zenith: {e}");
            std::process::exit(1);
        }
    };
    println!("{} — grammar {}", path.display(), syntax.name);
    for (i, runs) in hl.dump_scopes(syntax, &text, max_lines).iter().enumerate() {
        println!("{:>5}", i + 1);
        for r in runs {
            println!(
                "        {:<9} {:?}\n                  {}",
                format!("{:?}", r.tone),
                r.text,
                r.scopes.join(" > ")
            );
        }
    }
}

/// Headless parse: the same pipeline the window uses, printed as text. Handy for
/// checking tone assignment and the outline without a display.
fn dump(path: &std::path::Path) {
    let hl = syntax::Highlighter::new();
    let head = std::fs::read_to_string(path)
        .map(|s| s.lines().next().unwrap_or("").to_string())
        .unwrap_or_default();
    let syntax = hl.syntax_for(path, &head);
    println!("file    : {}", path.display());
    println!("grammar : {}", syntax.name);
    let readable = match syntax::read_readable(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("zenith: cannot read {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let (text, bytes, truncated) = match readable {
        syntax::Readable::Text {
            text,
            bytes,
            truncated,
            ..
        } => (text, bytes, truncated),
        syntax::Readable::Binary { bytes, reason } => {
            println!("binary  : {reason} ({bytes} bytes) — not readable as text");
            return;
        }
    };
    let doc = hl.parse(syntax, &text, bytes);
    println!("set in  : {}", doc.syntax);
    println!(
        "lines   : {} ({}{})",
        doc.lines.len(),
        folder::human_size(doc.bytes as u64),
        if truncated || doc.truncated {
            ", truncated"
        } else {
            ""
        }
    );

    let syms = outline::extract(path, &doc.syntax, &doc.lines);
    println!("outline : {} symbols", syms.len());
    for s in syms.iter().take(40) {
        println!(
            "  {:>6}  {}{} {} {}",
            s.line + 1,
            "  ".repeat(s.depth.min(6)),
            s.kind.glyph(&typography::build(0, 0, 0).marks),
            s.name,
            if s.detail.is_empty() {
                String::new()
            } else {
                format!("— {}", s.detail)
            }
        );
    }

    println!("\ntones (first 25 non-blank lines):");
    let mut shown = 0;
    for (idx, line) in doc.lines.iter().enumerate() {
        if line.is_blank() {
            continue;
        }
        if shown >= 25 {
            break;
        }
        shown += 1;
        let runs: Vec<String> = line
            .tokens
            .iter()
            .map(|t| format!("{:?}{{{}}}", t.tone, t.text.replace('\n', "\\n")))
            .collect();
        println!(
            "  {:>5} ind{} {:?} {}",
            idx + 1,
            line.indent,
            line.flow,
            runs.join(" ")
        );
    }
    let blank = doc.lines.iter().filter(|l| l.is_blank()).count();
    println!(
        "\nrhythm  : {} blank of {} lines",
        blank,
        doc.lines.len()
    );
}
