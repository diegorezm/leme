//! Characters on a page, text a PDF maps to nothing, and selections.

use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

use crate::recover::{Glyph, Recovery};
use crate::*;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CharBox {
    pub(crate) c: char,
    pub(crate) rect: NormRect,
}

/// A page's characters, and whether it also draws text PDFium reads as
/// blank (see `has_unreadable_text`).
pub(crate) fn char_boxes(
    doc: &PdfDocument,
    page: usize,
) -> Result<(Vec<CharBox>, bool), PdfiumError> {
    let pdf_page = load_page(doc, page)?;
    let size = (pdf_page.width().value, pdf_page.height().value);
    let text = pdf_page.text()?;
    let boxes = text
        .chars()
        .iter()
        .map(|ch| CharBox {
            c: ch.unicode_char().unwrap_or(' '),
            rect: ch
                .loose_bounds()
                .map(|r| normalize(r, size))
                .unwrap_or(NormRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }),
        })
        .collect::<Vec<_>>();
    let unreadable = has_unreadable_text(&pdf_page, &text, &boxes);
    Ok((boxes, unreadable))
}

/// A page's character boxes, shared so handing them out copies nothing.
pub(crate) type Chars = Rc<[CharBox]>;

/// A page's character boxes, from the cache of recently selected pages.
/// `recover` gives the glyphs of a page whose text PDFium can't read;
/// it's only called for pages that draw such text.
pub(crate) fn cached_chars(
    doc: &PdfDocument,
    cache: &mut VecDeque<(usize, Chars)>,
    page: usize,
    recover: &mut impl FnMut(usize) -> Vec<Glyph>,
) -> Chars {
    if !cache.iter().any(|(p, _)| *p == page) {
        let (mut boxes, unreadable) = char_boxes(doc, page).unwrap_or_default();
        if unreadable {
            boxes = merge_recovered(boxes, recover(page));
        }
        cache.push_front((page, boxes.into()));
        cache.truncate(CHAR_CACHE_PAGES);
    }
    cache
        .iter()
        .find(|(p, _)| *p == page)
        .map(|(_, boxes)| Rc::clone(boxes))
        .unwrap_or_else(|| Rc::new([]))
}

/// The document's recovery of unreadable text: not needed yet, loaded,
/// or not possible.
pub(crate) enum Recovering {
    Unloaded,
    Ready(Box<Recovery>),
    Failed,
}

impl Recovering {
    pub(crate) fn glyphs(
        &mut self,
        path: &Path,
        password: Option<&str>,
        page: usize,
    ) -> Vec<Glyph> {
        if let Recovering::Unloaded = self {
            *self = Recovery::load(path, password)
                .map_or(Recovering::Failed, |r| Recovering::Ready(Box::new(r)));
        }
        match self {
            Recovering::Ready(recovery) => recovery.glyphs(page),
            _ => Vec::new(),
        }
    }
}

/// Whether a page draws text PDFium reads as blank: a text object with
/// visible bounds, no character inside, and no text, as with fonts that
/// map glyphs to spaces. Asking PDFium for an object's text is slow
/// (about 20 µs), so only objects with no character inside are asked.
pub(crate) fn has_unreadable_text(
    pdf_page: &PdfPage,
    page_text: &PdfPageText,
    boxes: &[CharBox],
) -> bool {
    let (w, h) = (pdf_page.width().value, pdf_page.height().value);
    if !(w > 0.0 && h > 0.0) {
        return false;
    }
    // Centers of readable characters, sorted by height for range lookups.
    let mut centers: Vec<(f32, f32)> = boxes
        .iter()
        .filter(|b| !b.c.is_whitespace() && b.rect.width > 0.0)
        .map(|b| {
            (
                b.rect.y + b.rect.height / 2.0,
                b.rect.x + b.rect.width / 2.0,
            )
        })
        .collect();
    centers.sort_by(|a, b| a.0.total_cmp(&b.0));
    pdf_page.objects().iter().any(|object| {
        let Some(text) = object.as_text_object() else {
            return false;
        };
        let Ok(bounds) = object.bounds() else {
            return false;
        };
        if bounds.width().value <= 0.5 || bounds.height().value <= 0.5 {
            return false;
        }
        let r = normalize(bounds.to_rect(), (w, h));
        let from = centers.partition_point(|c| c.0 < r.y);
        let has_char = centers[from..]
            .iter()
            .take_while(|c| c.0 <= r.y + r.height)
            .any(|c| c.1 >= r.x && c.1 <= r.x + r.width);
        !has_char && page_text.for_object(text).trim().is_empty()
    })
}

pub(crate) fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n')
}

/// Horizontal space, as a page fraction, wider than letter spacing.
pub(crate) const WORD_GAP: f32 = 0.003;

/// Adds recovered glyphs to PDFium's characters. Glyphs PDFium already
/// has text for are dropped; the rest are grouped into runs (a formula),
/// which replace the blanks PDFium put there and go into reading order
/// after the text to their left on the same line.
pub(crate) fn merge_recovered(mut boxes: Vec<CharBox>, glyphs: Vec<Glyph>) -> Vec<CharBox> {
    let center = |r: &NormRect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let inside = |r: &NormRect, (x, y): (f32, f32)| {
        x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
    };
    let missing: Vec<Glyph> = glyphs
        .into_iter()
        .filter(|g| !g.c.is_whitespace())
        .filter(|g| {
            let c = center(&g.rect);
            !boxes
                .iter()
                .any(|b| !b.c.is_whitespace() && inside(&b.rect, c))
        })
        .collect();

    // Runs: glyphs next to each other on one line, in content order.
    let mut runs: Vec<Vec<Glyph>> = Vec::new();
    for glyph in missing {
        let joins = runs.last().and_then(|run| run.last()).is_some_and(|last| {
            let gap = glyph.rect.x - (last.rect.x + last.rect.width);
            let (_, cy) = center(&glyph.rect);
            let overlap = cy >= last.rect.y - last.rect.height / 2.0
                && cy <= last.rect.y + last.rect.height * 1.5;
            overlap && (-0.01..0.04).contains(&gap)
        });
        match runs.last_mut() {
            Some(run) if joins => run.push(glyph),
            _ => runs.push(vec![glyph]),
        }
    }

    for run in runs {
        let x0 = run.iter().map(|g| g.rect.x).fold(f32::MAX, f32::min);
        let x1 = run
            .iter()
            .map(|g| g.rect.x + g.rect.width)
            .fold(f32::MIN, f32::max);
        let y0 = run.iter().map(|g| g.rect.y).fold(f32::MAX, f32::min);
        let y1 = run
            .iter()
            .map(|g| g.rect.y + g.rect.height)
            .fold(f32::MIN, f32::max);
        let on_line = |b: &CharBox| {
            let (_, cy) = center(&b.rect);
            cy >= y0 && cy <= y1
        };
        // The blanks and line breaks PDFium made of the run. A line break
        // there is kept when it really ends the line: nothing on this line
        // follows it.
        let blank: Vec<bool> = (0..boxes.len())
            .map(|i| {
                let b = &boxes[i];
                let (cx, _) = center(&b.rect);
                let inside =
                    b.c.is_whitespace() && on_line(b) && cx > x0 + WORD_GAP && cx < x1 - WORD_GAP;
                let ends_line = is_break(b.c)
                    && !boxes[i + 1..]
                        .iter()
                        .find(|n| !is_break(n.c))
                        .is_some_and(on_line);
                inside && !ends_line
            })
            .collect();
        let mut index = 0;
        boxes.retain(|_| {
            index += 1;
            !blank[index - 1]
        });
        let at = boxes
            .iter()
            .rposition(|b| on_line(b) && !is_break(b.c) && center(&b.rect).0 <= x0)
            .map(|i| i + 1)
            .or_else(|| boxes.iter().position(on_line))
            .unwrap_or(boxes.len());
        let mut added: Vec<CharBox> = run
            .iter()
            .zip(recover::run_text(&run))
            .flat_map(|(g, text)| {
                let rect = g.rect;
                text.chars()
                    .map(move |c| CharBox { c, rect })
                    .collect::<Vec<_>>()
            })
            .collect();
        // Spaces where the run stands apart from the words around it.
        let spaced = |b: Option<&CharBox>, gap: f32| {
            b.is_some_and(|b| !b.c.is_whitespace() && on_line(b)) && gap > WORD_GAP
        };
        let before = at.checked_sub(1).and_then(|i| boxes.get(i));
        if spaced(
            before,
            before.map_or(0.0, |b| x0 - (b.rect.x + b.rect.width)),
        ) {
            let rect = NormRect {
                width: 0.0,
                ..run[0].rect
            };
            added.insert(0, CharBox { c: ' ', rect });
        }
        let after = boxes.get(at);
        if spaced(after, after.map_or(0.0, |b| b.rect.x - x1)) {
            let rect = NormRect {
                x: x1,
                width: 0.0,
                ..run[run.len() - 1].rect
            };
            added.push(CharBox { c: ' ', rect });
        }
        boxes.splice(at..at, added);
    }
    boxes
}

/// The first and last character of the line holding `chars[index]`: the
/// run of characters around it whose vertical centers overlap its own,
/// stopping at line breaks.
pub(crate) fn line_range(chars: &[CharBox], index: usize) -> (usize, usize) {
    let Some(anchor) = chars.get(index).map(|ch| ch.rect) else {
        return (index, index);
    };
    let center = anchor.y + anchor.height / 2.0;
    let same_line = |ch: &CharBox| {
        if ch.c == '\n' || ch.c == '\r' {
            return Some(false);
        }
        // Spaces often have no box; they don't end the line.
        if ch.rect.width <= 0.0 || ch.rect.height <= 0.0 {
            return None;
        }
        let other = ch.rect.y + ch.rect.height / 2.0;
        Some((center - other).abs() < anchor.height.max(ch.rect.height) / 2.0)
    };
    let mut a = index;
    while a > 0 && same_line(&chars[a - 1]) != Some(false) {
        a -= 1;
    }
    let mut b = index;
    while b + 1 < chars.len() && same_line(&chars[b + 1]) != Some(false) {
        b += 1;
    }
    (a, b)
}

/// Fills in the line each part starts on.
pub(crate) fn number_lines(
    mut parts: Vec<SelectionPart>,
    mut chars: impl FnMut(usize) -> Chars,
) -> Vec<SelectionPart> {
    for part in &mut parts {
        if let Some(first) = part.rects.first() {
            part.line = line_at(&chars(part.page), first.y + first.height / 2.0);
        }
    }
    parts
}

/// The one-based visual line at height `y` (page fraction): the number
/// of distinct text lines whose centers are at or above it. Lines side by
/// side, as in two columns, count once, so the number matches what a
/// reader counts down the page.
pub(crate) fn line_at(chars: &[CharBox], y: f32) -> u32 {
    let mut lines: Vec<(f32, f32)> = chars
        .iter()
        .filter(|ch| ch.rect.width > 0.0 && ch.rect.height > 0.0 && !ch.c.is_whitespace())
        .map(|ch| (ch.rect.y + ch.rect.height / 2.0, ch.rect.height))
        .collect();
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut count = 0;
    let mut last: Option<(f32, f32)> = None;
    for (center, height) in lines {
        if center > y + height / 4.0 {
            break;
        }
        match last {
            Some((c, h)) if center - c < h.max(height) / 2.0 => {}
            _ => {
                count += 1;
                last = Some((center, height));
            }
        }
    }
    count.max(1)
}

/// Selects text from one page point to another, possibly pages apart.
/// `chars` gives a page's character boxes.
pub(crate) fn select_pages(
    from: (usize, (f32, f32)),
    to: (usize, (f32, f32)),
    mut chars: impl FnMut(usize) -> Chars,
) -> Vec<SelectionPart> {
    if from.0 == to.0 {
        let boxes = chars(from.0);
        return select_text(&boxes, from.1, to.1)
            .map(|(rects, text)| SelectionPart {
                page: from.0,
                rects,
                text,
                line: 0,
            })
            .into_iter()
            .collect();
    }
    let (start, end) = if from.0 < to.0 {
        (from, to)
    } else {
        (to, from)
    };
    let last = end.0.min(start.0 + MAX_SELECT_PAGES - 1);
    (start.0..=last)
        .filter_map(|page| {
            let boxes = chars(page);
            let a = if page == start.0 {
                nearest_char(&boxes, start.1)?
            } else {
                0
            };
            let b = if page == end.0 {
                nearest_char(&boxes, end.1)?
            } else {
                boxes.len().checked_sub(1)?
            };
            let (rects, text) = select_range(&boxes, a, b)?;
            Some(SelectionPart {
                page,
                rects,
                text,
                line: 0,
            })
        })
        .collect()
}

/// Selects the characters between the ones nearest `from` and `to`.
/// Returns one rect per line and the selected text.
pub(crate) fn select_text(
    chars: &[CharBox],
    from: (f32, f32),
    to: (f32, f32),
) -> Option<(Vec<NormRect>, String)> {
    let a = nearest_char(chars, from)?;
    let b = nearest_char(chars, to)?;
    select_range(chars, a, b)
}

pub(crate) fn select_range(
    chars: &[CharBox],
    a: usize,
    b: usize,
) -> Option<(Vec<NormRect>, String)> {
    let range = chars.get(a.min(b)..=a.max(b))?;

    let mut rects: Vec<NormRect> = Vec::new();
    for ch in range
        .iter()
        .filter(|ch| !ch.c.is_whitespace() && ch.rect.width > 0.0)
    {
        let r = ch.rect;
        match rects.last_mut() {
            // Same line: vertical centers overlap, and the char is to the right.
            Some(line)
                if (line.y + line.height / 2.0 - (r.y + r.height / 2.0)).abs()
                    < line.height.max(r.height) / 2.0
                    && r.x >= line.x =>
            {
                let right = (line.x + line.width).max(r.x + r.width);
                let top = line.y.min(r.y);
                let bottom = (line.y + line.height).max(r.y + r.height);
                line.width = right - line.x;
                line.y = top;
                line.height = bottom - top;
            }
            _ => rects.push(r),
        }
    }
    let text: String = range.iter().map(|ch| ch.c).collect();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim().to_owned();
    (!rects.is_empty()).then_some((rects, text))
}

pub(crate) fn nearest_char(chars: &[CharBox], (x, y): (f32, f32)) -> Option<usize> {
    let distance = |r: &NormRect| {
        let dx = (r.x - x).max(x - (r.x + r.width)).max(0.0);
        let dy = (r.y - y).max(y - (r.y + r.height)).max(0.0);
        // Favor the same line: vertical distance weighs more.
        dx + dy * 4.0
    };
    chars
        .iter()
        .enumerate()
        .filter(|(_, ch)| ch.rect.width > 0.0)
        .min_by(|(_, a), (_, b)| distance(&a.rect).total_cmp(&distance(&b.rect)))
        .map(|(i, _)| i)
}

/// Converts a PDF rect (points, origin bottom left) to page fractions.
pub(crate) fn normalize(rect: PdfRect, (page_w, page_h): (f32, f32)) -> NormRect {
    if page_w <= 0.0 || page_h <= 0.0 {
        return NormRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
    }
    NormRect {
        x: rect.left().value / page_w,
        y: (page_h - rect.top().value) / page_h,
        width: rect.width().value / page_w,
        height: rect.height().value / page_h,
    }
}
