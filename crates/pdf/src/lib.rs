//! PDF access on a dedicated worker thread. PDFium never runs on the UI thread.
//!
//! The worker does one small unit of work at a time (one page render, one
//! page searched, one thumbnail) and checks for new requests in between, so
//! newer requests replace stale work instead of waiting behind it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;

use pdfium_render::prelude::*;

mod document;
mod find;
pub mod recover;
mod render;
mod text;
pub mod typeset;
mod worker;

pub use domain::geometry::NormRect;

pub use crate::document::bind;
#[allow(unused_imports)]
use crate::{document::*, find::*, render::*, text::*, worker::*};

/// How a rendered page is recolored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageTint {
    /// As the PDF draws it.
    #[default]
    None,
    /// Inverted lightness, for reading at night.
    Inverted,
    /// White becomes this RGB, the theme's paper.
    Paper([u8; 3]),
}

pub enum Request {
    Open {
        path: PathBuf,
        password: Option<String>,
    },
    /// Replaces the render queue: render `previews` quickly at a quarter
    /// of `width_px`, then `pages`, in order, `width_px` wide.
    Render {
        previews: Vec<usize>,
        pages: Vec<usize>,
        width_px: u32,
    },
    /// Replaces the thumbnail queue. Thumbnails run after renders and search.
    Thumbnails { pages: Vec<usize>, width_px: u32 },
    /// Starts a search over all pages, replacing any running one. An empty
    /// query just cancels.
    Search { id: u64, query: String },
    /// Selects the text between two points, each a page and a point in
    /// page fractions. The points may be on different pages.
    Select {
        from: (usize, (f32, f32)),
        to: (usize, (f32, f32)),
    },
    /// Selects the whole line of text nearest a point (double click).
    SelectLine { page: usize, point: (f32, f32) },
    /// How future renders color the page.
    SetTint(PageTint),
    /// Follows the link at a point on a page (page fractions), if any.
    LinkAt { page: usize, point: (f32, f32) },
    /// Searches the text of other documents, one page at a time, replacing
    /// any running file search. An empty query just cancels.
    SearchFiles {
        id: u64,
        query: String,
        paths: Vec<PathBuf>,
    },
    /// Finds the page where article `number` of a law starts ("Art. 37").
    FindArticle { number: u32 },
    /// Renders the first page of each document, `width_px` wide, as its
    /// cover; replaces any covers still to do.
    Covers { paths: Vec<PathBuf>, width_px: u32 },
    /// Finds where annotations' texts are in the document at `path`
    /// (rewritten since): id, text, and the page it was on.
    Relocate {
        path: PathBuf,
        items: Vec<(u64, String, usize)>,
    },
    /// Typesets a web page's text as a PDF at `out`, replacing it.
    Typeset {
        article: Box<web::html::Article>,
        source: String,
        out: PathBuf,
    },
}

/// One outline (table of contents) entry, flattened in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub depth: usize,
    pub page: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    Page(usize),
    /// An external http(s) or mailto link.
    Uri(String),
}

/// `B` is the bitmap type: the worker sends `Bitmap`s, which the receiver
/// may convert (with `map_bitmap`) before handing them to the UI thread.
pub enum Event<B = Bitmap> {
    /// `page_sizes` are in points, one per page.
    Opened {
        path: PathBuf,
        title: String,
        page_sizes: Vec<(f32, f32)>,
        outline: Vec<OutlineItem>,
    },
    /// The document is encrypted; `retry` is true if a password was wrong.
    PasswordNeeded {
        path: PathBuf,
        retry: bool,
    },
    /// Page labels (e.g. "iv"), one per page, empty when a page has none.
    /// Sent once, after the rest of the opening work.
    Labels(Vec<String>),
    Link(Option<LinkTarget>),
    Rendered {
        page: usize,
        width_px: u32,
        bitmap: B,
    },
    Thumbnail {
        page: usize,
        bitmap: B,
    },
    /// Matches on one page; sent only for pages with matches, in page order.
    SearchHits {
        id: u64,
        page: usize,
        matches: Vec<Vec<NormRect>>,
    },
    SearchDone {
        id: u64,
    },
    /// Result of a `Select`, one part per page; empty when nothing was
    /// selected.
    Selection(Vec<SelectionPart>),
    /// A page of another document that has the query, with the text around
    /// the first match.
    FileHit {
        id: u64,
        path: PathBuf,
        page: usize,
        snippet: String,
        count: usize,
    },
    FilesDone {
        id: u64,
    },
    /// Where an article starts, if found.
    Article {
        number: u32,
        page: Option<usize>,
    },
    /// Where annotations' texts were found: id, page, rects. Ids missing
    /// were not found.
    Relocated {
        path: PathBuf,
        found: Vec<(u64, usize, Vec<NormRect>)>,
    },
    /// A document's first page, and how many pages it has.
    Cover {
        path: PathBuf,
        bitmap: B,
        pages: usize,
    },
    /// A web page was typeset to `path`, or why not.
    Typeset {
        path: PathBuf,
        result: Result<(), String>,
    },
    Error(String),
}

impl<B> Event<B> {
    /// Converts the bitmap of a render or thumbnail, if any.
    pub fn map_bitmap<C>(self, f: impl FnOnce(B) -> C) -> Event<C> {
        match self {
            Event::Rendered {
                page,
                width_px,
                bitmap,
            } => Event::Rendered {
                page,
                width_px,
                bitmap: f(bitmap),
            },
            Event::Thumbnail { page, bitmap } => Event::Thumbnail {
                page,
                bitmap: f(bitmap),
            },
            Event::Cover {
                path,
                bitmap,
                pages,
            } => Event::Cover {
                path,
                bitmap: f(bitmap),
                pages,
            },
            Event::Opened {
                path,
                title,
                page_sizes,
                outline,
            } => Event::Opened {
                path,
                title,
                page_sizes,
                outline,
            },
            Event::PasswordNeeded { path, retry } => Event::PasswordNeeded { path, retry },
            Event::Labels(labels) => Event::Labels(labels),
            Event::Link(target) => Event::Link(target),
            Event::SearchHits { id, page, matches } => Event::SearchHits { id, page, matches },
            Event::SearchDone { id } => Event::SearchDone { id },
            Event::Selection(parts) => Event::Selection(parts),
            Event::FileHit {
                id,
                path,
                page,
                snippet,
                count,
            } => Event::FileHit {
                id,
                path,
                page,
                snippet,
                count,
            },
            Event::FilesDone { id } => Event::FilesDone { id },
            Event::Article { number, page } => Event::Article { number, page },
            Event::Typeset { path, result } => Event::Typeset { path, result },
            Event::Relocated { path, found } => Event::Relocated { path, found },
            Event::Error(message) => Event::Error(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionPart {
    pub page: usize,
    /// One rect per line.
    pub rects: Vec<NormRect>,
    pub text: String,
    /// One-based line on the page where the selection starts.
    pub line: u32,
}

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 pixels.
    pub rgba: Vec<u8>,
}

pub struct Worker {
    tx: Sender<Request>,
}

impl Worker {
    /// Spawns the worker. `on_event` is called from the worker thread.
    pub fn spawn(on_event: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || run(rx, on_event));
        Self { tx }
    }

    pub fn send(&self, request: Request) {
        // The worker only stops if PDFium failed to load, which was already reported.
        let _ = self.tx.send(request);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::recover::Glyph;

    #[test]
    fn file_search_ignores_case_and_accents() {
        let query: Vec<char> = "licitacao".chars().map(fold).collect();
        let text = "Lei 14.133. A LICITAÇÃO é o procedimento.\r\nOutra licitação aqui.";
        let (count, snippet) = find_in_text(text, &query).unwrap();
        assert_eq!(count, 2);
        assert!(snippet.starts_with("Lei 14.133. A LICITAÇÃO"));
        assert!(!snippet.contains('\r'));
        assert!(find_in_text("nada", &query).is_none());
        let long = format!("{}needle{}", "a ".repeat(60), " b".repeat(60));
        let (_, snippet) = find_in_text(&long, &['n', 'e', 'e', 'd', 'l', 'e']).unwrap();
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
    }

    fn boxes(text: &str, y: f32) -> Vec<CharBox> {
        text.chars()
            .enumerate()
            .map(|(i, c)| CharBox {
                c,
                rect: NormRect {
                    x: 0.1 + i as f32 * 0.02,
                    y,
                    width: 0.02,
                    height: 0.03,
                },
            })
            .collect()
    }

    #[test]
    fn selects_across_lines() {
        let mut chars = boxes("hello world", 0.1);
        chars.push(CharBox {
            c: '\n',
            rect: NormRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        });
        chars.extend(boxes("second", 0.2));
        // From inside "world" to the middle of "second".
        let (rects, text) = select_text(&chars, (0.23, 0.11), (0.15, 0.21)).unwrap();
        assert_eq!(text, "world\nsec");
        assert_eq!(rects.len(), 2);
        assert!((rects[0].x - 0.22).abs() < 1e-5);
        assert!((rects[0].width - 0.10).abs() < 1e-5);
        // Backwards drags select the same text.
        let (_, back) = select_text(&chars, (0.15, 0.21), (0.23, 0.11)).unwrap();
        assert_eq!(back, text);
        assert!(select_text(&[], (0.0, 0.0), (1.0, 1.0)).is_none());
    }

    #[test]
    fn selects_a_whole_line() {
        let mut chars = boxes("first line", 0.1);
        chars.push(CharBox {
            c: '\n',
            rect: NormRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        });
        chars.extend(boxes("second", 0.2));
        // A line without a break char ends where the height changes.
        chars.extend(boxes("third", 0.3));
        assert_eq!(line_range(&chars, 3), (0, 9));
        let (a, b) = line_range(&chars, 13);
        let (_, text) = select_range(&chars, a, b).unwrap();
        assert_eq!(text, "second");
        assert_eq!(line_range(&[], 0), (0, 0));
    }

    #[test]
    fn counts_visual_lines() {
        let mut chars = boxes("one", 0.1);
        chars.extend(boxes("two", 0.2));
        // A second column beside line two counts once.
        chars.extend(boxes("col", 0.205));
        chars.extend(boxes("three", 0.3));
        assert_eq!(line_at(&chars, 0.115), 1);
        assert_eq!(line_at(&chars, 0.215), 2);
        assert_eq!(line_at(&chars, 0.315), 3);
        assert_eq!(line_at(&[], 0.5), 1);
    }

    #[test]
    fn only_web_links_are_safe() {
        assert!(safe_uri("https://example.com"));
        assert!(safe_uri("MAILTO:a@b.c"));
        assert!(!safe_uri("file:///C:/Windows/System32/cmd.exe"));
        assert!(!safe_uri("javascript:alert(1)"));
        assert!(!safe_uri(" https://x.com"));
        assert!(!safe_uri("https://x\n.com"));
    }

    #[test]
    fn selects_across_pages() {
        let pages = [
            boxes("first page", 0.1),
            boxes("middle", 0.1),
            boxes("last page", 0.1),
        ];
        let chars = |page: usize| Chars::from(pages[page].clone());
        // From "page" on page 0 to "last" on page 2, dragged backwards.
        let parts = select_pages((2, (0.165, 0.11)), (0, (0.225, 0.11)), chars);
        let text: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, ["page", "middle", "last"]);
        assert_eq!(parts.iter().map(|p| p.page).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(select_pages((0, (0.0, 0.0)), (1, (0.0, 0.0)), |_| Chars::from([])).is_empty());
    }

    #[test]
    fn recovered_formulas_replace_the_blanks_pdfium_left() {
        let at = |c: char, x: f32, y: f32| CharBox {
            c,
            rect: NormRect {
                x,
                y,
                width: if c.is_whitespace() { 0.0 } else { 0.02 },
                height: 0.03,
            },
        };
        let glyph = |c: char, x: f32, y: f32, size: f32, baseline: f32| Glyph {
            c,
            rect: NormRect {
                x,
                y,
                width: 0.02,
                height: 0.03,
            },
            baseline,
            size,
        };
        let mut page = boxes("ab", 0.1);
        // PDFium's blanks inside the formula, including a bogus line break.
        page.extend([at(' ', 0.16, 0.1), at('\r', 0.17, 0.1), at('\n', 0.17, 0.1)]);
        page.extend([at(',', 0.25, 0.1), at('\r', 0.27, 0.1), at('\n', 0.27, 0.1)]);
        // A formula ending a line, after which PDFium's line break is real.
        page.extend(boxes("34", 0.3));
        page.extend([
            at('\r', 0.145, 0.3),
            at('\n', 0.145, 0.3),
            at('b', 0.1, 0.4),
        ]);
        let glyphs = vec![
            glyph('𝑥', 0.15, 0.1, 11.0, 0.125),
            glyph('2', 0.17, 0.095, 8.0, 0.115),
            glyph('=', 0.19, 0.1, 11.0, 0.125),
            glyph('0', 0.21, 0.1, 11.0, 0.125),
            // PDFium already reads this one.
            glyph('a', 0.10, 0.1, 11.0, 0.125),
            glyph('c', 0.15, 0.3, 11.0, 0.325),
            glyph('m', 0.17, 0.3, 11.0, 0.325),
        ];
        let text: String = merge_recovered(page, glyphs).iter().map(|b| b.c).collect();
        assert_eq!(text, "ab x²=0 ,\r\n34 cm\r\nb");
    }

    #[test]
    fn refined_search_skips_pages_without_the_shorter_query() {
        let mut first = SearchJob::new(1, "Foo".into(), 10, None);
        assert_eq!(first.pages, (0..10).collect::<Vec<_>>());
        // Stopped after page 5, with matches on 2 and 4.
        while first.next < 6 {
            first.next_page();
        }
        first.hits = vec![2, 4];
        let narrowed = SearchJob::new(2, "foobar".into(), 10, Some(first));
        assert_eq!(narrowed.pages, [2, 4, 6, 7, 8, 9]);
        let other = SearchJob::new(3, "baz".into(), 10, Some(narrowed));
        assert_eq!(other.pages.len(), 10);
        assert!(!refines("", "a"));
        assert_eq!(utf16le(&[b'i', 0, b'v', 0, 0, 0]), "iv");
        assert_eq!(squash("Hol-\r\nmes, É"), "holmesé");
    }

    #[test]
    fn huge_renders_are_capped() {
        assert_eq!(capped_width(1600, (595.0, 842.0)), 1600);
        let w = capped_width(100_000, (595.0, 842.0));
        let h = w as f32 * 842.0 / 595.0;
        assert!(w as f32 * h <= MAX_RENDER_PIXELS && w > 4000);
        // A very long, thin page is capped by area, not width.
        assert!(capped_width(2000, (100.0, 100_000.0)) < 200);
        // So is a very long one by its height.
        assert!(capped_width(2000, (100.0, 20_000.0)) as f32 * 200.0 <= MAX_RENDER_SIDE);
        assert_eq!(capped_width(0, (0.0, 0.0)), 1);
    }

    #[test]
    fn paper_tints_white_and_keeps_black() {
        let mut px = [255, 255, 255, 77, 0, 0, 0, 255, 128, 128, 128, 255];
        tint_paper(&mut px, [234, 223, 200]);
        assert_eq!(px[..4], [234, 223, 200, 77]);
        assert_eq!(px[4..8], [0, 0, 0, 255]);
        assert_eq!(px[8..], [117, 112, 100, 255]);
    }

    /// Timings for a large document: `LEME_BENCH_PDF=<file> cargo test
    /// --release bench_document -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_document() {
        use std::time::Instant;
        let (Ok(pdfium), Ok(path)) = (bind(), std::env::var("LEME_BENCH_PDF")) else {
            eprintln!("needs PDFium and LEME_BENCH_PDF");
            return;
        };
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;

        let t = Instant::now();
        let (doc, sizes) = open(&pdfium, Path::new(&path), None).unwrap();
        println!("open, {} pages: {:.1} ms", sizes.len(), ms(t));
        let t = Instant::now();
        let items = outline(&doc).len();
        println!("outline, {items} items: {:.1} ms", ms(t));

        let pages: Vec<usize> = (0..sizes.len())
            .step_by((sizes.len() / 20).max(1))
            .collect();
        for (label, width, forms) in [
            ("page render 1600 px", 1600, true),
            ("thumbnail 180 px", 180, false),
        ] {
            let t = Instant::now();
            for &page in &pages {
                render(&doc, page, width, PageTint::None, forms).unwrap();
            }
            println!("{label}: {:.1} ms avg", ms(t) / pages.len() as f64);
        }
        let mut bitmap = render(&doc, 0, 1600, PageTint::None, true).unwrap();
        let t = Instant::now();
        invert(&mut bitmap.rgba);
        println!("invert 1600 px: {:.2} ms", ms(t));
        let t = Instant::now();
        tint_paper(&mut bitmap.rgba, [234, 223, 200]);
        println!("paper tint 1600 px: {:.2} ms", ms(t));
        let t = Instant::now();
        for page in 0..sizes.len() {
            search_page(&doc, page, "holmes", None).unwrap();
        }
        println!("search all pages: {:.0} ms", ms(t));
        // Typing "holmes" letter by letter, each search narrowing the last.
        let t = Instant::now();
        let mut last: Option<SearchJob> = None;
        let mut texts = vec![None; sizes.len()];
        for end in 1..="holmes".len() {
            let mut job = SearchJob::new(0, "holmes"[..end].into(), sizes.len(), last.take());
            while let Some(page) = job.next_page() {
                let text = texts.get_mut(page);
                if !search_page(&doc, page, &job.query, text)
                    .unwrap()
                    .is_empty()
                {
                    job.hits.push(page);
                }
            }
            last = Some(job);
        }
        println!("typed search, 6 queries: {:.0} ms", ms(t));
        let t = Instant::now();
        for page in 0..sizes.len() {
            search_page(&doc, page, "watson", texts.get_mut(page)).unwrap();
        }
        println!("search again, text cached: {:.0} ms", ms(t));
        let t = Instant::now();
        let labels = page_labels(&pdfium, Path::new(&path), None, sizes.len());
        println!("page labels: {:.1} ms", ms(t));
        for (page, label) in labels.iter().enumerate().step_by(97) {
            let slow = load_page(&doc, page)
                .ok()
                .and_then(|p| p.label().map(str::to_owned));
            assert_eq!(*label, slow.unwrap_or_default());
        }
    }

    /// Needs PDFium; skipped when the library can't be loaded.
    #[test]
    fn opens_and_renders_fixture() {
        let Ok(pdfium) = bind() else {
            eprintln!("PDFium unavailable, skipping");
            return;
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/two-pages.pdf");
        let (doc, sizes) = open(&pdfium, &path, None).unwrap();
        assert!(outline(&doc).is_empty());
        assert_eq!(page_labels(&pdfium, &path, None, 2), vec![String::new(); 2]);
        assert_eq!(link_at(&doc, 0, (0.5, 0.5)), None);
        assert_eq!(sizes, vec![(200.0, 300.0); 2]);
        let bitmap = render(&doc, 1, 100, PageTint::None, true).unwrap();
        assert_eq!((bitmap.width, bitmap.height), (100, 150));
        assert_eq!(bitmap.rgba.len(), 100 * 150 * 4);
        assert!(render(&doc, 5, 100, PageTint::None, true).is_err());
        assert_eq!(
            search_page(&doc, 0, "anything", None).unwrap(),
            Vec::<Vec<_>>::new()
        );
    }
}
