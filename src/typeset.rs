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

/// A word placed on a line: x offset from the line start.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub text: String,
    pub face: Face,
    pub strike: bool,
    pub width: f32,
}

/// Breaks runs into lines no wider than `width`, greedily, measuring
/// with `measure(text, face)` at size 1. Words wider than a line are
/// split. Pure, so it is tested without PDFium.
pub fn break_lines(
    runs: &[(String, Face, bool)],
    size: f32,
    width: f32,
    measure: &mut dyn FnMut(&str, Face) -> f32,
) -> Vec<Vec<Piece>> {
    let mut lines: Vec<Vec<Piece>> = Vec::new();
    let mut line: Vec<Piece> = Vec::new();
    let mut x = 0.0;
    let mut pending_space = false;
    for (text, face, strike) in runs {
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
                        line.push(piece(x, &part, *face, *strike, pw));
                        lines.push(std::mem::take(&mut line));
                        x = 0.0;
                        part = c.to_string();
                    }
                }
                let pw = measure(&part, *face) * size;
                line.push(piece(x, &part, *face, *strike, pw));
                x += pw;
            } else {
                line.push(piece(x, word, *face, *strike, w));
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

fn piece(x: f32, text: &str, face: Face, strike: bool, width: f32) -> Piece {
    Piece {
        x,
        text: text.to_owned(),
        face,
        strike,
        width,
    }
}

/// Joins neighboring pieces of one style into one, so each line is a few
/// text objects rather than one per word.
fn merge(line: Vec<Piece>, space: &mut dyn FnMut(Face) -> f32) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for p in line {
        match out.last_mut() {
            Some(last) if last.face == p.face && last.strike == p.strike => {
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
    let push_block = |runs: Vec<(String, Face, bool)>,
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
    if !article.title.is_empty() {
        push_block(
            vec![(article.title.clone(), Face::Bold, false)],
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
    push_block(
        vec![(source.to_owned(), Face::Regular, false)],
        &source_style,
        MUTED,
        4.0,
        &mut lines,
        &mut metrics,
    );

    for (i, block) in article.blocks.iter().enumerate() {
        let st = style(block.kind);
        let runs = block_runs(block, &st);
        let gap = if i == 0 { 14.0 } else { st.space_before };
        if block.kind == Kind::ListItem {
            let mut runs = runs;
            runs.insert(0, ("• ".to_owned(), Face::Regular, false));
            push_block(runs, &st, INK, gap, &mut lines, &mut metrics);
        } else {
            push_block(runs, &st, INK, gap, &mut lines, &mut metrics);
        }
    }

    // Lay lines onto pages.
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
        if page.is_none() || y - gap - height < MARGIN {
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
        let baseline = y + line.size * (LEADING - 1.0) / 2.0 + line.size * 0.2;
        let color = PdfColor::new(line.color.0, line.color.1, line.color.2, 255);
        for p in &line.pieces {
            let x = MARGIN + line.indent + p.x;
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
    Ok(pages)
}

fn block_runs(block: &Block, st: &Style) -> Vec<(String, Face, bool)> {
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
            (r.text.clone(), face, r.strike)
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
        let runs = vec![("aaa bbb ccc dddd".to_owned(), Face::Regular, false)];
        let lines = break_lines(&runs, 2.0, 8.0, &mut fixed);
        assert_eq!(texts(&lines), ["aaa bbb", "ccc dddd"]);
        assert_eq!(lines[0][1].x, 4.0);
    }

    #[test]
    fn styles_join_without_inventing_spaces() {
        let runs = vec![
            ("Art. ".to_owned(), Face::Bold, false),
            ("5".to_owned(), Face::Regular, false),
            ("º caput".to_owned(), Face::Regular, true),
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
        let runs = vec![("abcdefghij".to_owned(), Face::Regular, false)];
        let lines = break_lines(&runs, 2.0, 4.0, &mut fixed);
        assert_eq!(texts(&lines), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn merges_same_style_pieces() {
        let runs = vec![
            ("one two".to_owned(), Face::Regular, false),
            (" three".to_owned(), Face::Bold, false),
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
        let article = crate::html::extract(&crate::html::decode(&bytes));
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
    }
}
