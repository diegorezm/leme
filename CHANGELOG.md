# Changelog

All notable changes to Leme (called dpdf until 1.1.0). Versions follow [Semantic Versioning](https://semver.org/).

## 2.2.0 — 2026-11-25

### Added

- **Visual page transforms**: rotate pages clockwise or counter-clockwise
  (`Ctrl+Shift+R`) and crop empty white margins (`Ctrl+M`), persisted per
  document without modifying the PDF.
- **Two-page view** (`Alt+2`): side-by-side spreads, remembered per document.
- **Jump history** (`Alt+Left` / `Alt+Right`): step back and forward across
  outline, link, search, and go-to-page jumps.
- **Auto-reload on disk change** (`F5` to reload manually; `auto_reload = true`
  by default): detects PDFs rebuilt on disk (e.g. from LaTeX) when the window
  regains focus and reloads in place while keeping your scroll position.
- **PDF content fingerprints**: moving or renaming a PDF keeps its reading
  position, notes, and study time attached automatically.
- **Search inside recovered formulas**: `Ctrl+F` now also matches text in
  Word equations and fonts that PDFium maps to blanks.
- **Persistent cover cache**: library document covers are cached on disk
  (keyed by path, modification time, and width) so libraries open instantly.

### Fixed

- **Thumbnails on tab switch**: thumbnails no longer disappear when switching
  tabs with margin cropping or rotation active, and recent tabs keep their
  rendered thumbnails cached for instant switching.
- **Multi-column text selection**: dragging across lines in multi-column
  layouts no longer merges highlights across the column gutter.
- **Stale worker events**: generation counters drop outdated renders,
  thumbnails, selections, links, and labels after rapid tab switches or tint
  changes.
- **Broken annotation files**: a syntax error in a notes file is reported
  calmly instead of being overwritten on the next change.
- **Linux & web pages**: open external links and files with `xdg-open` on
  Linux; preserve indentation and line breaks in preformatted code blocks;
  atomic file writes and proper YAML escaping on Markdown export.

### Performance

- **Native cropped & rotated renders**: PDFium renders transformed pages
  directly into a single output-sized buffer and packs RGB in place, avoiding
  margin rasterization and software pixel copies.
- **Faster tab switches and scrolling**: PDF fingerprinting runs once on the
  worker thread instead of the UI thread; redundant `state.txt` writes on tab
  switch are eliminated; outline selection updates incrementally during scroll;
  and keyboard scrolling no longer rebuilds page/thumbnail models per step.
- **Faster search and libraries**: per-page search caches skip character-box
  scans on readable pages, session search reduces allocations and stat calls,
  and library cover/icon scans avoid redundant disk work.

## 2.1.0 — 2026-11-16

### Added

- **Add to session panel** (`Ctrl+O`): choose a file, paste a PDF's path,
  or pick a recent document from any session.
- **Web pages as documents.** Paste an address and Leme saves the page as
  a PDF you can highlight, annotate, and search offline. *Update page from
  the web* downloads it again; highlights find their text again, and those
  whose text is gone are listed last in the notes.
- **Laws from planalto.gov.br** come without the letterhead, with Títulos,
  Capítulos, and Seções as the outline, and revoked text hidden (*Show or
  hide revoked text*, or `show_revoked = true`).
- **Links in saved pages** work: jumps within the page, other laws open in
  Leme, the rest in the browser.
- **Libraries**: named collections of documents shared by every session,
  each with an icon or your own picture. A library opens as a page of
  cards with each document's first page and progress, and a *Continue
  reading* card (`Ctrl+Shift+L`, or from the home screen); its add card
  opens the `Ctrl+O` panel to add files, recent documents, pages, or laws.
  Settings › Libraries shows them as cards to create, rename, pick an icon
  or picture, and remove documents. Saved laws go into *Leis* on their own.
- **Law shortcuts**: `cf 37`, `cpc art. 1.015`, or a saved law's number
  (`8112 41`) open the law at the article, downloading it if needed. Add
  your own under `[aliases]` in `config.toml`.
- **Resizable side panel** (drag its edge; double-click resets) and
  scrollbars in thumbnails, outline, and notes.

### Changed

- `Ctrl+O` opens the Add to session panel instead of the file dialog,
  which is its first entry (*Choose file…*, also bindable as
  `choose-file`).

### Performance

Measured on a 1493-page book (`cargo test --release bench_document`):

| | 2.0.1 | 2.1.0 |
|---|---|---|
| Render of a 1600 px page | 28.4 ms | 13.8 ms |
| Render of a 1600 px page, night colors | ~31 ms | 12.1 ms |

- PDFium draws straight into one buffer, packed to RGB and recolored in a
  single pass; the UI gets it with one copy instead of a pixel loop.
- Search starts at the page you're reading and wraps around, so the next
  match appears at once in long books.
- Search results update only the page they're on, instead of rebuilding
  every visible page for each page with a match.
- Library pictures are decoded once per update instead of twice.

## 2.0.1 — 2026-11-16

### Changed

- **Settings button** back in the header, right of the command search.
- **Settings icons** back next to each section in the settings sidebar.

## 2.0.0 — 2026-11-15

### Changed

- **New design.** Reworked header, reader, palette, settings, and widgets,
  with a new Mare theme and refreshed built-in themes.
- **Highlight colors.** New defaults: ochre *Important*, vermilion *Doubt*,
  verdigris *Definition*, indigo *Example*, madder *Review*.
- **Notes filter.** From *All*, click a color to see it alone, then add or
  remove others.

### Added

- **Paper page colors.** `page_colors = "paper"` tints the PDF's white with the
  theme's new `page` color.
- **Session search from the search panel** (*All tabs*), grouped by document.

## 1.2.1 — 2026-11-15

### Changed

- **One window.** Opening a PDF while Leme is running adds it as a tab to the
  current session and brings the window forward, instead of starting a
  second Leme.

### Added

- **Linux support (from source).** Leme builds and runs on Linux: settings in
  `~/.config/leme`, caches in `~/.cache/leme`. No Linux release yet.

## 1.2.0 — 2026-11-11

### Changed

- **dpdf is now Leme.** The app, executable, and repository are renamed.
  Settings, notes, and history move from `%APPDATA%\dpdf` to
  `%APPDATA%\leme` on first start; the installer replaces the old copy and
  its "Open with" entry. Installed copies of dpdf update to Leme as usual.
- **New look.** Noite (dark, the new default) and Velino (light) themes: warm
  ink and vellum with one rubric red. Older themes are still available.
- **Markdown export.** Block ids are now `^leme-<id>` and the tag is `leme`;
  links to `^dpdf-` ids in older exports stop resolving after re-exporting.

## 1.1.0 — 2026-11-10

### Changed

- **Header icons.** Fit width, fit page, zoom, and settings are icons; hovering
  shows the name and its shortcut.
- **Notifications.** Messages like "Downloading dpdf…" or "Copied" appear in
  the bottom-right corner instead of the header. Click × or press `Esc` to
  dismiss.

## 1.0.5 — 2026-11-10

### Added

- **Highlight colors with a meaning.** Each color has a name (yellow
  *Important*, red *Trap*, and so on), set in `config.toml` under
  `[color_names]`. Keys `1`–`5` highlight the selection or recolor the
  clicked highlight, and page thumbnails show the colors on each page.
- **Notes filter.** The notes panel can hide colors.
- **Section progress.** Outline sections can be marked read and reviewed
  (click the box or `Ctrl+L`); the outline and the home screen show how far
  each document is.
- **Export sheet** (`Ctrl+E`): a Markdown summary grouped by section and
  color, the Obsidian format, a file Anki imports as cards, or CSV.
- **Study timer** in the header, counting the time you actually read, per
  day and document (`Ctrl+T` pauses it; `study_timer = false` hides it).
- **Home screen** lists the session's tabs with progress, notes, and time.
- **Session search** (`Ctrl+Shift+F`) across every tab, ignoring accents.
- **Brazilian Portuguese interface**, the new default; English stays
  available in Settings → General → Language (`language` in config.toml).
- **Go to article**: `art. 37` in Go to page jumps to that article.

## 1.0.4 — 2026-11-09

Version 1.0.3 was skipped: its performance work ships here, together with
the formula fix.

### Fixed

- **Copying formulas from Word-made PDFs.** Equations exported by Word use
  a Cambria Math subset whose text map turns most symbols into spaces, so
  copying `x² − 4x + 2 = 0` gave blanks (in other readers too). dpdf now
  reads the glyphs the page actually draws and names them through the
  font: the copy embedded in the PDF first, then the same font installed on
  Windows. Exponents and indices come out as `²`, `₁`, and so on; math
  italic letters as plain ones. Two-dimensional layouts such as stacked
  fractions are copied in reading order, without fraction bars.

### Performance

Measured on a 1088-page book (`cargo test --release bench_document`):

| | 1.0.2 | 1.0.4 |
|---|---|---|
| Dark-mode inversion of a 1600 px page | 6.6 ms | 2.7 ms |
| Page labels after opening | 305 ms | 0.6 ms |
| Second search through the whole book | ~1200 ms | 577 ms |

- Page pixels are converted on the render thread; the UI thread no longer
  copies a full page per render.
- Pages and thumbnails are kept as RGB: a quarter less memory each.
- A finished render updates only its own page instead of rebuilding every
  visible page.
- Rendered pages share a 192 MB memory budget, evicting the pages farthest
  from view first; tabs kept in the background hold only the pages that
  were on screen.
- One render is capped at 24 megapixels and 16384 px per side, so deep
  zoom no longer asks for 300 MB bitmaps; past that the page is stretched.
- Pages not rendered yet show their thumbnail, or a quarter-size preview,
  instead of a blank.
- Zooming in quick steps (Ctrl+wheel, a held key) renders only the last
  step.
- The reading position is saved at most every two seconds while scrolling,
  instead of on every page; closing, switching tabs, and quitting still
  save at once.
- Typing a search narrows the previous one, and pages already searched are
  skipped quickly when they can't match.
- Page labels are read without loading every page, and kept per document.
- Text selection no longer copies a page's characters on every drag event.
- Thumbnails skip form fields.

## 1.0.2 — 2026-11-09

### Performance

- Switching between recent tabs is instant: the document stays open and
  its rendered pages and exact position are kept.

## 1.0.1 — 2026-11-09

### Added

- Update check against GitHub releases, with an offer to install (off with
  `check_updates = false`).
- Double-click selects a line; note tooltips; Obsidian-friendly notes
  export.
- A header button to reopen the closed sidebar.

### Fixed

- Keyboard focus is kept after closing the sidebar with its button.

## 1.0.0 — 2026-11-09

First release.
