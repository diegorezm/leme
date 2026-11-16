# Leme documentation

How to install, use, customize, and build leme. For an overview, see the [README](README.md).

## Install

Download `leme-<version>-setup.exe` (installs per user, no admin needed, optionally adds Leme to "Open with" for PDFs) or the portable `.zip` from [Releases](https://github.com/diegorezm/leme/releases).

## Shortcuts

The defaults. All of them can be changed (see [Customizing](#customizing)), and Settings → Keybindings lists every action.

| Shortcut                                 | Action                                                                  |
| ---------------------------------------- | ----------------------------------------------------------------------- |
| `Ctrl+O`                                 | Add to session: a file, a recent document, or a web page's address      |
| `Ctrl+K` / `Ctrl+Shift+P`                | Command palette                                                         |
| `Ctrl+F`                                 | Search (`Enter` / `Shift+Enter` for next / previous match)              |
| `Ctrl+Shift+F`                           | Search the text of every tab in the session                             |
| `Ctrl+P`                                 | Go to page, or to an article of a law (`art. 37`)                       |
| `Ctrl++` / `Ctrl+-` / `Ctrl`+wheel       | Zoom in / out                                                           |
| `Ctrl+0` or `Ctrl+1` / `Ctrl+2`          | Fit width / fit page                                                    |
| `Ctrl+B`                                 | Toggle sidebar (drag its edge to resize; double-click it to reset)      |
| `Ctrl+Shift+T` / `O` / `A`               | Sidebar: thumbnails / outline / notes                                   |
| `Ctrl+.`                                 | Focus mode                                                              |
| `F11`                                    | Fullscreen                                                              |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` / `Ctrl+W` | Next / previous / close tab                                             |
| `Alt+Home`                               | Home screen (tabs stay open)                                            |
| `Ctrl+Shift+S` / `Ctrl+Shift+N`          | Switch session / new session                                            |
| `←` `→` `PgUp` `PgDn` / `↑` `↓`          | Previous / next page / scroll                                           |
| `Home` / `End`                           | First / last page                                                       |
| `Ctrl+C` / `Ctrl+H` / `Ctrl+N`           | Copy / highlight / add a note to the selection                          |
| `1` … `5`                                | Highlight the selection in that color, or recolor the clicked highlight |
| `Ctrl+Z` / `Ctrl+Y`                      | Undo / redo annotation changes                                          |
| `Ctrl+L`                                 | Mark the current outline section read, then reviewed once, twice        |
| `Ctrl+E`                                 | Export notes: Markdown, Anki, or CSV                                    |
| `Ctrl+T`                                 | Pause / resume the study timer                                          |
| `Ctrl+,`                                 | Settings                                                                |
| `Esc`                                    | Close overlay, clear selection, leave fullscreen or focus mode          |

With **vim keys** on: `j` `k` `h` `l` scroll, `Ctrl+D` / `Ctrl+U` half screen, `J` / `K` next / previous page, `gg` / `G` first / last page, `/` search, `n` / `N` next / previous match, `:` palette, `gt` / `gT` tabs, `zi` `zo` `zw` `zp` zoom, `y` copy, `u` / `Ctrl+R` undo / redo, `t` sidebar.

## Studying

Leme has a few tools for studying, for exams for example, that stay out of the way when you don't use them.

- **Colors with a meaning.** The five highlight colors have names, shown in the notes panel and in exports: ochre _Important_ (`1`), vermilion _Doubt_ (`2`), verdigris _Definition_ (`3`), indigo _Example_ (`4`), madder _Review_ (`5`). Rename them and change their shades in Settings → Reading and study → Highlights (or in `config.toml`, below). Page thumbnails show which colors a page has, and the notes panel filters by color: from _All_, click a color to see it alone, then click others to add or remove them.
- **Progress.** In the outline, click the box next to a section (or press `Ctrl+L` in it) to mark it read, then reviewed once and twice. The outline shows how much of the document you've read; the home screen shows it for every tab in the session, with notes and study time.
- **Export.** `Ctrl+E` exports the notes in the colors the notes panel shows: as a Markdown summary grouped by section and color (or the Obsidian format, grouped by page), as a file Anki imports as cards (_File › Import_; the note is the front, the quote the back, color and document are tags), or as CSV. Reviewing the cards is left to Anki or whatever app you use.
- **Study time.** The header shows how long you've read today. Time counts while you use Leme (pauses longer than five minutes don't count) and is kept per day and document in `study.txt`. Click it or press `Ctrl+T` to pause. To remove it, turn off Settings → Reading → Study timer (`study_timer = false`); then nothing is counted.
- **Session search and law articles.** `Ctrl+Shift+F` (or _All tabs_ in the search panel) searches every tab in the session at once, listing the pages found under each document, ignoring case and accents (`licitacao` finds _licitação_). In Go to page, `art. 37` jumps to where that article starts.

## Web pages

Paste a page's address in Add to session (`Ctrl+O`) and choose *Save page as document*: Leme downloads it once, keeps only the text (headings, paragraphs, bold, italic, struck-through), and saves it as a PDF you can highlight, annotate, and search like any other, offline. *Update page from the web* (palette) downloads it again into the same file. Highlights and notes then find their text again; any whose text is gone (revoked, or changed) keep their note and are listed last in the notes panel, under *Not found after updating*. Pages that build their text with JavaScript or need a login can't be read this way; Leme says so instead of saving an empty page. Saved pages live in the data folder under `pages\`.

Links in the page stay clickable: those within it jump there, others open in the browser, and links to other laws on planalto.gov.br open in Leme (the saved copy, or downloaded on the spot). Laws from planalto.gov.br get extra care: the letterhead and link lines above the law go, the law's name becomes the title, and its Títulos, Capítulos, and Seções become the outline. Revoked (struck-through) text is hidden; *Show or hide revoked text* (palette) switches the open law, without downloading it again, and `show_revoked = true` in `config.toml` shows it struck through by default.

## Libraries

Libraries are named collections of documents, shared by every session: saved pages and PDFs from disk, each library with an icon or a picture of yours. They're listed on the home screen; `Ctrl+Shift+L` (*Libraries…*) opens one to search its documents, add or remove the open document, change its icon or picture, rename, or delete it (its documents stay). *Add document to library…* puts the open document in one. Laws saved from planalto.gov.br go into *Leis* on their own. The palette and Add to session find documents in every library.

**Law shortcuts.** In the palette or Add to session, type a law's shortcut and maybe an article: `cf 37`, `cpc art. 1.015`, `clt`. Built in: `cf`, `cc`, `cp`, `cpc`, `cpp`, `clt`, `ctn`, `cdc`; a law not saved yet is downloaded from planalto.gov.br. A saved law's number works too (`8112 41`). Add your own in `config.toml`:

```toml
[aliases]
lgpd = "https://www.planalto.gov.br/ccivil_03/_ato2015-2018/2018/lei/l13709.htm"
```

Links between laws that point to an article (`…l8112.htm#art5`) open at that article.

## Customizing

Everything lives in `%APPDATA%\leme\` (Settings → General → Data folder):

| File                 | What                                                                        |
| -------------------- | --------------------------------------------------------------------------- |
| `config.toml`        | Preferences and keybindings. Settings writes here too.                      |
| `themes\*.toml`      | Your themes. Start from any file in this repo's [`themes/`](themes) folder. |
| `annotations\*.toml` | Highlights and notes, one file per document.                                |
| `libraries.toml`     | Libraries: names, icons, pictures, and their documents.                     |
| `state.txt`          | Recent files, reading positions, and sessions with their tabs.              |
| `study.txt`          | Study time per day and document.                                            |

Font, keybindings, and more in `config.toml`:

```toml
language = "pt-BR"               # interface language: "pt-BR" (default) or "en"
font = "Segoe UI Variable Text"  # any installed font, e.g. "Inter"
font_size = 14                   # 10 to 22
density = "comfortable"          # compact, comfortable, spacious
corners = "rounded"              # sharp, rounded, round
thumbnail_size = "medium"        # small, medium, large
vim = true
check_updates = false            # don't ask GitHub for new releases on startup
sidebar_button = false           # hide the header button that reopens the sidebar
notes_folder = 'C:\Users\me\Vault\PDF notes'  # Ctrl+E saves here, no dialog
study_timer = false              # hide the study time in the header

[color_names]                    # what each highlight color means
red = "Pegadinha"
green = "Jurisprudência"

[highlight_colors]               # their shades
red = "#ff5c5c"

[keys]
open = ["ctrl+o"]
first-page = ["home", "g g"]   # sequences are space-separated
search = []                     # an empty list unbinds
```

A theme:

```toml
name = "My Theme"
dark = true             # missing colors fall back to the built-in dark theme
background = "#101418"
accent = "#ff9e64"
page = "#eadfc8"        # the paper: what white becomes with page_colors = "paper"
```

After editing, run **Reload config and themes** from the palette.

## Building from source

### Requirements

- Rust (stable) via [rustup](https://rustup.rs)
- Windows with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`) and the Visual Studio C++ Build Tools

### Run

```sh
cargo run                      # then Ctrl+O to pick files
cargo run -- a.pdf b.pdf       # or open some directly, one tab each
```

PDF rendering uses [PDFium](https://pdfium.googlesource.com/pdfium/). The first build downloads a prebuilt `pdfium.dll` (from [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)) using the `curl` and `tar` that ship with Windows, caches it in `target/pdfium/`, and copies it next to `leme.exe`. At runtime the app only contacts github.com to check for updates (`check_updates` in `config.toml`).

To distribute Leme, ship `leme.exe` and `pdfium.dll` together from `target/release/`. Pushing a `v*` tag runs the `release` workflow, which builds both, a zip, and an [Inno Setup](https://jrsoftware.org/isinfo.php) installer (`installer/leme.iss`), and publishes a GitHub release.

### Develop

```sh
cargo test                                  # tests
cargo fmt --all                             # format
cargo clippy --all-targets -- -D warnings   # lint
cargo build --release                       # optimized binary: target/release/leme.exe
```

## Performance

Measured on a 1,088-page PDF (release build, Windows 11, `cargo test --release bench_document -- --ignored --nocapture` with `LEME_BENCH_PDF` set):

|                             |                                                                  |
| --------------------------- | ---------------------------------------------------------------- |
| Open document               | 9 ms                                                             |
| Read outline (88 entries)   | 2 ms                                                             |
| Render a page at 1600 px    | 12 ms average                                                    |
| Render a thumbnail          | 2 ms average                                                     |
| Search every page           | 1.2 s (in the background, one page at a time; results stream in) |
| Read page labels            | 0.3 s (in the background, after everything else)                 |
| Window shown after launch   | ~30 ms warm, ~0.4 s cold                                         |
| Memory, no document         | ~130 MB working set                                              |
| Memory, 1,088-page document | ~210 MB working set                                              |
| `leme.exe` / `pdfium.dll`   | 12 MB / 7.2 MB                                                   |

Idle memory is above the 50 MB goal in AGENTS.md and hasn't been profiled yet (the OpenGL renderer and GPU driver are the likely bulk). With a document open it grows only by the bounded page and thumbnail caches.
