//! Typesets an [`Article`] into a PDF with PDFium's built-in Times fonts,
//! so a saved web page reads, highlights, and searches like any other
//! document. Runs on the PDF worker, which owns PDFium.

use std::collections::HashMap;
use std::path::Path;

use pdfium_render::prelude::*;

use crate::html::{Article, Block, Kind, Run};

/// A5-like page: narrow enough that fit-width reads comfortably.
const PAGE_W: f32 = 420.0;
const PAGE_H: f32 = 595.0;
const MARGIN: f32 = 42.0;
const BODY: f32 = 11.0;
const LEADING: f32 = 1.4;
const INDENT: f32 = 14.0;
const INK: (u8, u8, u8) = (26, 26, 26);
const MUTED: (u8, u8, u8) = (110, 110, 110);
/// Link text: a dark blue that still reads as text on paper and inverted.
const LINK: (u8, u8, u8) = (30, 78, 150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

impl Face {
    fn of(run: &Run, bold: bool) -> Face {
        match (run.bold || bold, run.italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Bold,
            (false, true) => Face::Italic,
            (true, true) => Face::BoldItalic,
        }
    }
}

/// A run of text in one style, to break into lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub face: Face,
    pub strike: bool,
    /// An address, or `#id` within the document.
    pub link: Option<String>,
}

impl Span {
    pub fn plain(text: impl Into<String>, face: Face) -> Self {
        Span {
            text: text.into(),
            face,
            strike: false,
            link: None,
        }
    }
}

/// A word placed on a line: x offset from the line start.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub text: String,
    pub face: Face,
    pub strike: bool,
    pub width: f32,
    pub link: Option<String>,
}

/// Breaks runs into lines no wider than `width`, greedily, measuring
/// with `measure(text, face)` at size 1. Words wider than a line are
/// split. Pure, so it is tested without PDFium.
pub fn break_lines(
    runs: &[Span],
    size: f32,
    width: f32,
    measure: &mut dyn FnMut(&str, Face) -> f32,
) -> Vec<Vec<Piece>> {
    let mut lines: Vec<Vec<Piece>> = Vec::new();
    let mut line: Vec<Piece> = Vec::new();
    let mut x = 0.0;
    let mut pending_space = false;
    for span in runs {
        let (text, face) = (&span.text, &span.face);
        let starts_with_space = text.starts_with(char::is_whitespace);
        if starts_with_space {
            pending_space = true;
        }
        let mut words = text.split_whitespace().peekable();
        while let Some(word) = words.next() {
            let space = if pending_space && !line.is_empty() {
                measure(" ", *face) * size
            } else {
                0.0
            };
            let w = measure(word, *face) * size;
            if x + space + w > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                x = 0.0;
            } else {
                x += space;
            }
            if w > width {
                // Too long for any line: split by characters.
                let mut part = String::new();
                for c in word.chars() {
                    part.push(c);
                    if measure(&part, *face) * size > width - x && part.chars().count() > 1 {
                        part.pop();
                        let pw = measure(&part, *face) * size;
                        line.push(piece(x, &part, span, pw));
                        lines.push(std::mem::take(&mut line));
                        x = 0.0;
                        part = c.to_string();
                    }
                }
                let pw = measure(&part, *face) * size;
                line.push(piece(x, &part, span, pw));
                x += pw;
            } else {
                line.push(piece(x, word, span, w));
                x += w;
            }
            pending_space = words.peek().is_some();
        }
        pending_space =
            text.ends_with(char::is_whitespace) || (pending_space && text.trim().is_empty());
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn piece(x: f32, text: &str, span: &Span, width: f32) -> Piece {
    Piece {
        x,
        text: text.to_owned(),
        face: span.face,
        strike: span.strike,
        width,
        link: span.link.clone(),
    }
}

/// Joins neighboring pieces of one style into one, so each line is a few
/// text objects rather than one per word.
fn merge(line: Vec<Piece>, space: &mut dyn FnMut(Face) -> f32) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for p in line {
        match out.last_mut() {
            Some(last) if last.face == p.face && last.strike == p.strike && last.link == p.link => {
                let gap = p.x - (last.x + last.width);
                let spaces = if gap > 0.01 {
                    (gap / space(p.face)).round().max(1.0) as usize
                } else {
                    0
                };
                last.text.push_str(&" ".repeat(spaces));
                last.text.push_str(&p.text);
                last.width = p.x + p.width - last.x;
            }
            _ => out.push(p),
        }
    }
    out
}

struct Style {
    size: f32,
    bold: bool,
    indent: f32,
    space_before: f32,
    mono: bool,
    italic: bool,
}

fn style(kind: Kind) -> Style {
    let base = Style {
        size: BODY,
        bold: false,
        indent: 0.0,
        space_before: BODY * 0.6,
        mono: false,
        italic: false,
    };
    match kind {
        Kind::Heading(1) => Style {
            size: 17.0,
            bold: true,
            space_before: 16.0,
            ..base
        },
        Kind::Heading(2) => Style {
            size: 14.5,
            bold: true,
            space_before: 14.0,
            ..base
        },
        Kind::Heading(_) => Style {
            size: 12.5,
            bold: true,
            space_before: 11.0,
            ..base
        },
        Kind::ListItem => Style {
            indent: INDENT,
            space_before: 3.0,
            ..base
        },
        Kind::Quote => Style {
            indent: INDENT,
            italic: true,
            ..base
        },
        Kind::Code => Style {
            size: 8.5,
            mono: true,
            space_before: 6.0,
            ..base
        },
        Kind::Paragraph => base,
    }
}

/// Measures text through PDFium, caching each character's advance.
struct Metrics {
    advances: HashMap<(char, Face), f32>,
}

impl Metrics {
    fn width(&mut self, doc: &PdfDocument, fonts: &Fonts, text: &str, face: Face) -> f32 {
        text.chars()
            .map(|c| {
                *self
                    .advances
                    .entry((c, face))
                    .or_insert_with(|| advance(doc, fonts.get(face), c))
            })
            .sum()
    }
}

/// The advance of `c` at size 1: "x c x" minus "x x", since an object's
/// bounds are its ink, not its advance.
fn advance(doc: &PdfDocument, font: PdfFontToken, c: char) -> f32 {
    let bounds = |s: &str| {
        PdfPageTextObject::new(doc, s, font, PdfPoints::new(100.0))
            .and_then(|o| o.width())
            .map(|w| w.value / 100.0)
            .unwrap_or(0.5)
    };
    (bounds(&format!("x{c}x")) - bounds("xx")).max(0.0)
}

struct Fonts {
    regular: PdfFontToken,
    bold: PdfFontToken,
    italic: PdfFontToken,
    bold_italic: PdfFontToken,
    mono: PdfFontToken,
}

impl Fonts {
    fn get(&self, face: Face) -> PdfFontToken {
        match face {
            Face::Regular => self.regular,
            Face::Bold => self.bold,
            Face::Italic => self.italic,
            Face::BoldItalic => self.bold_italic,
            Face::Mono => self.mono,
        }
    }
}

/// One line ready to draw.
struct Line {
    pieces: Vec<Piece>,
    size: f32,
    indent: f32,
    color: (u8, u8, u8),
    /// Extra space above it.
    gap: f32,
    /// An outline entry starts here: level and title.
    mark: Option<(u8, String)>,
    /// Ids that `#id` links point to.
    anchors: Vec<String>,
}

/// Writes `article` as a PDF at `out`. `source` is shown under the title.
pub fn write(
    pdfium: &Pdfium,
    article: &Article,
    source: &str,
    out: &Path,
) -> Result<usize, PdfiumError> {
    let mut doc = pdfium.create_new_pdf()?;
    let fonts = {
        let f = doc.fonts_mut();
        Fonts {
            regular: f.times_roman(),
            bold: f.times_bold(),
            italic: f.times_italic(),
            bold_italic: f.times_bold_italic(),
            mono: f.courier(),
        }
    };
    let mut metrics = Metrics {
        advances: HashMap::new(),
    };
    let text_width = PAGE_W - 2.0 * MARGIN;

    let mut lines: Vec<Line> = Vec::new();
    let push_block = |runs: Vec<Span>,
                      st: &Style,
                      color,
                      first_gap: f32,
                      lines: &mut Vec<Line>,
                      metrics: &mut Metrics| {
        let width = text_width - st.indent;
        let broken = break_lines(&runs, st.size, width, &mut |t, f| {
            metrics.width(&doc, &fonts, t, f)
        });
        for (i, l) in broken.into_iter().enumerate() {
            let pieces = merge(l, &mut |f| metrics.width(&doc, &fonts, " ", f) * st.size);
            lines.push(Line {
                pieces,
                size: st.size,
                indent: st.indent,
                color,
                gap: if i == 0 { first_gap } else { 0.0 },
                mark: None,
                anchors: Vec::new(),
            });
        }
    };

    let title_style = Style {
        size: 19.0,
        bold: true,
        indent: 0.0,
        space_before: 0.0,
        mono: false,
        italic: false,
    };
    let titled = article
        .blocks
        .first()
        .is_some_and(|b| b.text() == article.title);
    if !article.title.is_empty() && !titled {
        push_block(
            vec![Span::plain(article.title.clone(), Face::Bold)],
            &title_style,
            INK,
            0.0,
            &mut lines,
            &mut metrics,
        );
    }
    let source_style = Style {
        size: 8.0,
        ..style(Kind::Paragraph)
    };
    // The address goes under the title, which is either the page's
    // title or its first block.
    let source_line = vec![Span::plain(source, Face::Regular)];
    if !titled {
        push_block(
            source_line.clone(),
            &source_style,
            MUTED,
            4.0,
            &mut lines,
            &mut metrics,
        );
    }

    for (i, block) in article.blocks.iter().enumerate() {
        if titled && i == 1 {
            push_block(
                source_line.clone(),
                &source_style,
                MUTED,
                4.0,
                &mut lines,
                &mut metrics,
            );
        }
        let st = style(block.kind);
        let runs = block_runs(block, &st);
        // Space between the title block (title and address) and the text.
        let gap = if i == usize::from(titled) {
            14.0
        } else {
            st.space_before
        };
        let first = lines.len();
        if block.kind == Kind::ListItem {
            let mut runs = runs;
            runs.insert(0, Span::plain("• ", Face::Regular));
            push_block(runs, &st, INK, gap, &mut lines, &mut metrics);
        } else {
            push_block(runs, &st, INK, gap, &mut lines, &mut metrics);
        }
        if let Some(line) = lines.get_mut(first) {
            if let Kind::Heading(level) = block.kind {
                line.mark = Some((level, block.text()));
            }
            line.anchors = block.anchors.clone();
        }
    }

    // Lay lines onto pages.
    let mut marks: Vec<Mark> = Vec::new();
    let mut links: Vec<LinkBox> = Vec::new();
    let mut anchors: HashMap<String, (usize, f32)> = HashMap::new();
    let mut pages = 0;
    let mut y = 0.0;
    let mut page: Option<PdfPage> = None;
    for line in lines {
        let height = line.size * LEADING;
        let gap = if page.is_some() && y < PAGE_H - MARGIN - 0.1 {
            line.gap
        } else {
            0.0
        };
        // Keep a heading with the lines after it.
        let needed = if line.mark.is_some() {
            height * 3.0
        } else {
            height
        };
        if page.is_none() || y - gap - needed < MARGIN {
            if let Some(mut done) = page.take() {
                done.regenerate_content()?;
            }
            let mut new = doc
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::Custom(
                    PdfPoints::new(PAGE_W),
                    PdfPoints::new(PAGE_H),
                ))?;
            // Regenerating the content stream after every object makes a
            // long page quadratic; do it once, when the page is full.
            new.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
            page = Some(new);
            pages += 1;
            y = PAGE_H - MARGIN;
        } else {
            y -= gap;
        }
        y -= height;
        let Some(page) = page.as_mut() else { continue };
        if let Some((level, title)) = line.mark {
            marks.push(Mark {
                level,
                title,
                page: pages - 1,
                top: y + height,
            });
        }
        for id in line.anchors {
            anchors.entry(id).or_insert((pages - 1, y + height));
        }
        let baseline = y + line.size * (LEADING - 1.0) / 2.0 + line.size * 0.2;
        for p in &line.pieces {
            let x = MARGIN + line.indent + p.x;
            let ink = if p.link.is_some() { LINK } else { line.color };
            let color = PdfColor::new(ink.0, ink.1, ink.2, 255);
            if let Some(target) = &p.link {
                links.push(LinkBox {
                    page: pages - 1,
                    rect: [x, y, x + p.width, y + height],
                    target: target.clone(),
                });
            }
            let mut object = page.objects_mut().create_text_object(
                PdfPoints::new(x),
                PdfPoints::new(baseline),
                &p.text,
                fonts.get(p.face),
                PdfPoints::new(line.size),
            )?;
            object.set_fill_color(color)?;
            if p.strike {
                let mid = baseline + line.size * 0.3;
                page.objects_mut().create_path_object_line(
                    PdfPoints::new(x),
                    PdfPoints::new(mid),
                    PdfPoints::new(x + p.width),
                    PdfPoints::new(mid),
                    color,
                    PdfPoints::new(line.size * 0.06),
                )?;
            }
        }
    }
    if let Some(mut done) = page.take() {
        done.regenerate_content()?;
    }
    if pages == 0 {
        doc.pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(
                PdfPoints::new(PAGE_W),
                PdfPoints::new(PAGE_H),
            ))?;
        pages = 1;
    }
    doc.save_to_file(out)?;
    drop(doc);
    // PDFium can't write bookmarks or links; lopdf adds them to the saved
    // file. A failure leaves the document without them, still readable.
    let _ = add_navigation(out, &marks, &links, &anchors);
    Ok(pages)
}

/// A link's clickable area: page, [x1, y1, x2, y2] in points, target.
struct LinkBox {
    page: usize,
    rect: [f32; 4],
    target: String,
}

/// Writes link annotations: addresses open as URIs, `#id` links go to
/// where that id is in the document.
fn add_links(
    doc: &mut lopdf::Document,
    pages: &[lopdf::ObjectId],
    links: &[LinkBox],
    anchors: &HashMap<String, (usize, f32)>,
) -> Result<(), lopdf::Error> {
    use lopdf::{Dictionary, Object, StringFormat};
    let mut per_page: HashMap<usize, Vec<Object>> = HashMap::new();
    for link in links {
        let mut annot = Dictionary::new();
        annot.set("Type", "Annot");
        annot.set("Subtype", "Link");
        annot.set(
            "Rect",
            link.rect
                .iter()
                .map(|v| Object::Real(*v))
                .collect::<Vec<_>>(),
        );
        annot.set(
            "Border",
            vec![Object::Integer(0), Object::Integer(0), Object::Integer(0)],
        );
        if let Some(id) = link.target.strip_prefix('#') {
            let Some(&(page, top)) = anchors.get(id) else {
                continue;
            };
            let Some(&page) = pages.get(page) else {
                continue;
            };
            annot.set(
                "Dest",
                vec![
                    Object::Reference(page),
                    "XYZ".into(),
                    Object::Null,
                    Object::Real(top),
                    Object::Null,
                ],
            );
        } else {
            let mut action = Dictionary::new();
            action.set("S", "URI");
            action.set(
                "URI",
                Object::String(link.target.clone().into_bytes(), StringFormat::Literal),
            );
            annot.set("A", action);
        }
        let id = doc.add_object(annot);
        per_page
            .entry(link.page)
            .or_default()
            .push(Object::Reference(id));
    }
    for (page, annots) in per_page {
        let Some(&page_id) = pages.get(page) else {
            continue;
        };
        if let Ok(dict) = doc.get_object_mut(page_id).and_then(Object::as_dict_mut) {
            dict.set("Annots", annots);
        }
    }
    Ok(())
}

/// A heading's place, for the outline.
struct Mark {
    level: u8,
    title: String,
    /// Zero-based page, and the heading's top in PDF points.
    page: usize,
    top: f32,
}

/// Writes `marks` into the PDF at `path` as its outline (bookmarks),
/// nested by level.
fn add_navigation(
    path: &Path,
    marks: &[Mark],
    links: &[LinkBox],
    anchors: &HashMap<String, (usize, f32)>,
) -> Result<(), lopdf::Error> {
    use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
    if marks.is_empty() && links.is_empty() {
        return Ok(());
    }
    let mut doc = Document::load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    add_links(&mut doc, &pages, links, anchors)?;
    if marks.is_empty() {
        doc.save(path)?;
        return Ok(());
    }
    let root_id = doc.new_object_id();
    let ids: Vec<ObjectId> = marks.iter().map(|_| doc.new_object_id()).collect();

    // Parent of each mark: the nearest earlier mark with a smaller level.
    let mut parent: Vec<Option<usize>> = Vec::with_capacity(marks.len());
    let mut stack: Vec<usize> = Vec::new();
    for (i, m) in marks.iter().enumerate() {
        while stack.last().is_some_and(|&j| marks[j].level >= m.level) {
            stack.pop();
        }
        parent.push(stack.last().copied());
        stack.push(i);
    }
    let children =
        |p: Option<usize>| -> Vec<usize> { (0..marks.len()).filter(|&i| parent[i] == p).collect() };
    let title = |text: &str| {
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        Object::String(bytes, StringFormat::Hexadecimal)
    };

    let mut descendants = vec![0i64; marks.len()];
    for i in (0..marks.len()).rev() {
        if let Some(p) = parent[i] {
            descendants[p] += descendants[i] + 1;
        }
    }
    for (i, m) in marks.iter().enumerate() {
        let siblings = children(parent[i]);
        let at = siblings.iter().position(|&s| s == i).unwrap_or(0);
        let kids = children(Some(i));
        let mut dict = Dictionary::new();
        dict.set("Title", title(&m.title));
        dict.set("Parent", parent[i].map_or(root_id, |p| ids[p]));
        if at > 0 {
            dict.set("Prev", ids[siblings[at - 1]]);
        }
        if let Some(&next) = siblings.get(at + 1) {
            dict.set("Next", ids[next]);
        }
        if let (Some(&first), Some(&last)) = (kids.first(), kids.last()) {
            dict.set("First", ids[first]);
            dict.set("Last", ids[last]);
            dict.set("Count", Object::Integer(descendants[i]));
        }
        if let Some(&page) = pages.get(m.page) {
            dict.set(
                "Dest",
                vec![
                    Object::Reference(page),
                    "XYZ".into(),
                    Object::Null,
                    Object::Real(m.top),
                    Object::Null,
                ],
            );
        }
        doc.objects.insert(ids[i], Object::Dictionary(dict));
    }
    let top = children(None);
    let mut root = Dictionary::new();
    root.set("Type", "Outlines");
    if let (Some(&first), Some(&last)) = (top.first(), top.last()) {
        root.set("First", ids[first]);
        root.set("Last", ids[last]);
    }
    root.set("Count", Object::Integer(marks.len() as i64));
    doc.objects.insert(root_id, Object::Dictionary(root));
    let catalog = doc.catalog_mut()?;
    catalog.set("Outlines", root_id);
    catalog.set("PageMode", "UseOutlines");
    doc.save(path)?;
    Ok(())
}

fn block_runs(block: &Block, st: &Style) -> Vec<Span> {
    block
        .runs
        .iter()
        .map(|r| {
            let face = if st.mono {
                Face::Mono
            } else {
                let mut face = Face::of(r, st.bold);
                if st.italic {
                    face = match face {
                        Face::Bold | Face::BoldItalic => Face::BoldItalic,
                        _ => Face::Italic,
                    };
                }
                face
            };
            Span {
                text: r.text.clone(),
                face,
                strike: r.strike,
                link: r.link.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(text: &str, _: Face) -> f32 {
        text.chars().count() as f32 * 0.5
    }

    fn texts(lines: &[Vec<Piece>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|p| p.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn breaks_greedily() {
        // Size 2: each character is 1pt wide, a space 1pt.
        let runs = vec![Span::plain("aaa bbb ccc dddd", Face::Regular)];
        let lines = break_lines(&runs, 2.0, 8.0, &mut fixed);
        assert_eq!(texts(&lines), ["aaa bbb", "ccc dddd"]);
        assert_eq!(lines[0][1].x, 4.0);
    }

    #[test]
    fn styles_join_without_inventing_spaces() {
        let runs = vec![
            Span::plain("Art. ", Face::Bold),
            Span::plain("5", Face::Regular),
            Span {
                strike: true,
                ..Span::plain("º caput", Face::Regular)
            },
        ];
        let lines = break_lines(&runs, 2.0, 100.0, &mut fixed);
        let line = &lines[0];
        assert_eq!(line[0].text, "Art.");
        // "Art." ends at 4, a space, then "5" at 5, and "º" right after it.
        assert_eq!(line[1].x, 5.0);
        assert_eq!(line[2].x, 6.0);
        assert!(line[2].strike);
    }

    #[test]
    fn splits_words_longer_than_a_line() {
        let runs = vec![Span::plain("abcdefghij", Face::Regular)];
        let lines = break_lines(&runs, 2.0, 4.0, &mut fixed);
        assert_eq!(texts(&lines), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn merges_same_style_pieces() {
        let runs = vec![
            Span::plain("one two", Face::Regular),
            Span::plain(" three", Face::Bold),
        ];
        let lines = break_lines(&runs, 2.0, 100.0, &mut fixed);
        let merged = merge(lines[0].clone(), &mut |_| 1.0);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "one two");
        assert_eq!(merged[1].text, "three");
    }

    /// Typesets a saved HTML page: `LEME_PAGE_HTML=<file> cargo test
    /// typeset_page -- --ignored --nocapture`. Writes `<file>.pdf`.
    #[test]
    #[ignore]
    fn typeset_page() {
        let Ok(file) = std::env::var("LEME_PAGE_HTML") else {
            return;
        };
        let Ok(pdfium) = crate::pdf::bind() else {
            eprintln!("PDFium not available; skipping");
            return;
        };
        let bytes = std::fs::read(&file).unwrap();
        let started = std::time::Instant::now();
        let url = std::env::var("LEME_PAGE_URL").unwrap_or_default();
        let article = crate::html::read_page(&bytes, &url, false);
        let read = started.elapsed();
        let out = std::path::PathBuf::from(format!("{file}.pdf"));
        let pages = write(&pdfium, &article, "https://example.com", &out).unwrap();
        println!(
            "{:?}: {} blocks, {} words, {pages} pages; read {read:?}, total {:?}",
            article.title,
            article.blocks.len(),
            article.word_count(),
            started.elapsed()
        );
        let doc = pdfium.load_pdf_from_file(&out, None).unwrap();
        let outline: Vec<String> = doc
            .bookmarks()
            .iter()
            .take(12)
            .map(|b| b.title().unwrap_or_default())
            .collect();
        println!("outline: {outline:?}");
    }
}
