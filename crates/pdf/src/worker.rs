//! The worker thread: what it keeps open, its queues, and its loop, one
//! small unit of work at a time.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use domain::config::{cache_dir, fnv1a, write_atomic_bytes};
use domain::geometry::ViewTransform;

use crate::*;

const COVER_MAGIC: &[u8; 4] = b"LCOV";
const COVER_HEADER_LEN: usize = 40;

fn modified_nanos(path: &Path) -> Option<u128> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
}

fn cover_cache_path(path: &Path) -> Option<PathBuf> {
    let key = fnv1a(path.to_string_lossy().as_bytes());
    Some(
        cache_dir()?
            .join("covers")
            .join(format!("{key:016x}.cover")),
    )
}

fn read_cached_cover(cache_path: &Path, mtime_ns: u128, req_width: u32) -> Option<(Bitmap, usize)> {
    let bytes = std::fs::read(cache_path).ok()?;
    if bytes.len() < COVER_HEADER_LEN || &bytes[0..4] != COVER_MAGIC {
        return None;
    }
    let stored_mtime = u128::from_le_bytes(bytes[4..20].try_into().ok()?);
    let stored_req_w = u32::from_le_bytes(bytes[20..24].try_into().ok()?);
    let width = u32::from_le_bytes(bytes[24..28].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[28..32].try_into().ok()?);
    let pages = u64::from_le_bytes(bytes[32..40].try_into().ok()?) as usize;
    if stored_mtime != mtime_ns || stored_req_w != req_width {
        return None;
    }
    let expected = width as usize * height as usize * 3;
    if bytes.len() != COVER_HEADER_LEN + expected {
        return None;
    }
    Some((
        Bitmap {
            width,
            height,
            rgb: bytes[COVER_HEADER_LEN..].to_vec(),
        },
        pages,
    ))
}

fn write_cached_cover(
    cache_path: &Path,
    mtime_ns: u128,
    req_width: u32,
    bitmap: &Bitmap,
    pages: usize,
) {
    let mut out = Vec::with_capacity(COVER_HEADER_LEN + bitmap.rgb.len());
    out.extend_from_slice(COVER_MAGIC);
    out.extend_from_slice(&mtime_ns.to_le_bytes());
    out.extend_from_slice(&req_width.to_le_bytes());
    out.extend_from_slice(&bitmap.width.to_le_bytes());
    out.extend_from_slice(&bitmap.height.to_le_bytes());
    out.extend_from_slice(&(pages as u64).to_le_bytes());
    out.extend_from_slice(&bitmap.rgb);
    let _ = write_atomic_bytes(cache_path, &out);
}

#[derive(Default)]
pub(crate) struct Queue {
    /// Pages to render rough and small first, so they show at once.
    pub(crate) previews: VecDeque<usize>,
    pub(crate) pages: VecDeque<usize>,
    pub(crate) width_px: u32,
}

/// Previews are this many times narrower than the full render.
pub(crate) const PREVIEW_DIVISOR: u32 = 4;

/// A pending selection: drag start and end, as pages and points.
pub(crate) type SelectJob = ((usize, (f32, f32)), (usize, (f32, f32)));

/// Documents kept open, so switching between tabs doesn't reparse them.
/// PDFium loads pages lazily, so an idle open document is cheap.
pub(crate) const OPEN_DOCUMENTS: usize = 4;

/// An open document and what's needed to hand it out again.
pub(crate) struct OpenDoc<'a> {
    pub(crate) path: PathBuf,
    /// To notice the file changing on disk.
    pub(crate) modified: Option<std::time::SystemTime>,
    pub(crate) doc: PdfDocument<'a>,
    pub(crate) page_sizes: Vec<(f32, f32)>,
    pub(crate) outline: Vec<OutlineItem>,
    /// Kept in memory only, to read page labels.
    pub(crate) password: Option<String>,
    /// Page labels, once read.
    pub(crate) labels: Option<Vec<String>>,
    /// Cached content box for margin cropping (`Some(None)` = scanned and has no margins).
    pub(crate) detected_crop: Option<Option<NormRect>>,
    /// For text the PDF draws but maps to nothing; loaded on first need.
    pub(crate) recovery: Recovering,
    /// Each page's text, squashed (see `squash`), once a search has read
    /// it. Later searches skip pages that can't match without asking
    /// PDFium. A few KB per page.
    pub(crate) texts: Vec<Option<String>>,
}

pub(crate) fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Pages a selection may span; bounds the work of a wild drag.
pub(crate) const MAX_SELECT_PAGES: usize = 50;
/// Pages whose character boxes are kept while dragging.
pub(crate) const CHAR_CACHE_PAGES: usize = 8;

/// A search in progress, or the last one, kept to narrow the next.
pub(crate) struct SearchJob {
    pub(crate) id: u64,
    pub(crate) query: String,
    /// Pages to search, ascending.
    pub(crate) pages: Vec<usize>,
    /// Index into `pages` of the next page to search.
    pub(crate) next: usize,
    /// Pages searched so far that had matches.
    pub(crate) hits: Vec<usize>,
}

impl SearchJob {
    /// A search of all `page_count` pages, from `from` onward and then
    /// wrapping, or, when `query` refines the `previous` one (typing more
    /// letters), only of the pages that query matched plus those it had not
    /// reached yet.
    pub(crate) fn new(
        id: u64,
        query: String,
        page_count: usize,
        from: usize,
        previous: Option<SearchJob>,
    ) -> Self {
        let from = from.min(page_count);
        let pages = match previous {
            Some(prev) if refines(&prev.query, &query) => {
                let mut pages = prev.hits;
                pages.extend_from_slice(prev.pages.get(prev.next..).unwrap_or_default());
                pages
            }
            _ => (from..page_count).chain(0..from).collect(),
        };
        Self {
            id,
            query,
            pages,
            next: 0,
            hits: Vec::new(),
        }
    }

    pub(crate) fn next_page(&mut self) -> Option<usize> {
        let page = self.pages.get(self.next).copied();
        self.next += 1;
        page
    }

    pub(crate) fn is_done(&self) -> bool {
        self.next >= self.pages.len()
    }
}

/// Lowercase letters and digits only. If PDFium finds a query in a page,
/// the squashed query is in the squashed page text, whatever spaces,
/// hyphens, or line breaks PDFium matched across.
pub(crate) fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether every match of `new` contains a match of `old`, so pages
/// without `old` can't have `new` either. Search is case-insensitive.
pub(crate) fn refines(old: &str, new: &str) -> bool {
    !old.is_empty() && new.to_lowercase().contains(&old.to_lowercase())
}

/// A search through other documents' text.
pub(crate) struct FilesJob<'a> {
    pub(crate) id: u64,
    pub(crate) query: Vec<char>,
    pub(crate) paths: VecDeque<PathBuf>,
    /// The document being searched, the next page in it, and the document
    /// itself once a page's text isn't cached.
    pub(crate) current: Option<(PathBuf, usize, Option<PdfDocument<'a>>)>,
    pub(crate) hits: usize,
}

/// Page texts of documents searched before, so searching again while
/// typing reads no PDF. Bounded by document count; a page's text is a few
/// KB.
#[derive(Default)]
pub(crate) struct FileTexts {
    /// Least recently searched first.
    pub(crate) docs: VecDeque<(PathBuf, Option<std::time::SystemTime>, Vec<Option<String>>)>,
}

pub(crate) const CACHED_FILE_TEXTS: usize = 12;

impl FileTexts {
    /// The cached pages of `path`, made current; `None` if not cached or
    /// the file changed.
    pub(crate) fn pages(&mut self, path: &Path) -> Option<&mut Vec<Option<String>>> {
        let i = self.docs.iter().position(|(p, _, _)| p == path)?;
        let entry = self.docs.remove(i)?;
        if entry.1 != modified(path) {
            return None;
        }
        self.docs.push_back(entry);
        self.docs.back_mut().map(|(_, _, pages)| pages)
    }

    /// Cached pages without re-checking disk metadata (for per-page steps
    /// after `pages()` validated the file at document start).
    pub(crate) fn pages_unchecked(&mut self, path: &Path) -> Option<&mut Vec<Option<String>>> {
        self.docs
            .iter_mut()
            .rfind(|(p, _, _)| p == path)
            .map(|(_, _, pages)| pages)
    }

    pub(crate) fn insert(&mut self, path: &Path, page_count: usize) {
        self.docs.retain(|(p, _, _)| p != path);
        if self.docs.len() >= CACHED_FILE_TEXTS {
            self.docs.pop_front();
        }
        self.docs
            .push_back((path.to_owned(), modified(path), vec![None; page_count]));
    }
}

/// Results a file search stops at.
pub(crate) const MAX_FILE_HITS: usize = 200;
/// Characters of context on each side of a file search match.
pub(crate) const SNIPPET_CONTEXT: usize = 50;

pub(crate) struct ArticleJob {
    pub(crate) number: u32,
    pub(crate) next: usize,
}

pub(crate) fn run<B>(rx: Receiver<Request>, convert: fn(Bitmap) -> B, on_event: impl Fn(Event<B>)) {
    let pdfium = match bind() {
        Ok(pdfium) => pdfium,
        Err(err) => {
            on_event(Event::Error(format!("PDFium could not be loaded: {err}")));
            return;
        }
    };
    // Most recently opened first; the front one is the live document.
    let mut documents: VecDeque<OpenDoc> = VecDeque::new();
    let mut renders = Queue::default();
    let mut thumbs = Queue::default();
    let mut search: Option<SearchJob> = None;
    let mut last_search: Option<SearchJob> = None;
    let mut select: Option<SelectJob> = None;
    // Character boxes of recently selected pages, reused while dragging.
    let mut char_cache: VecDeque<(usize, Chars)> = VecDeque::new();
    let mut tint = PageTint::None;
    let mut transform = ViewTransform::default();
    let mut doc_gen: u64 = 0;
    let mut render_gen: u64 = 0;
    // Whether the live document's page labels are still to be sent.
    let mut labels_due = false;
    let mut link: Option<(usize, (f32, f32))> = None;
    let mut select_line: Option<(usize, (f32, f32))> = None;
    let mut crop_job: Option<(u64, usize)> = None;
    let mut files: Option<FilesJob> = None;
    let mut file_texts = FileTexts::default();
    let mut article: Option<ArticleJob> = None;
    let mut covers: (VecDeque<PathBuf>, u32) = (VecDeque::new(), 0);

    loop {
        // Work on the live document the reader is waiting for, and work
        // that can wait for it.
        let urgent = !documents.is_empty()
            && (!renders.pages.is_empty()
                || !renders.previews.is_empty()
                || search.is_some()
                || select.is_some()
                || select_line.is_some()
                || link.is_some()
                || crop_job.is_some()
                || article.is_some());
        let background = files.is_some()
            || !covers.0.is_empty()
            || (!documents.is_empty() && (!thumbs.pages.is_empty() || labels_due));
        let idle = !urgent && !background;
        let request = if idle {
            match rx.recv() {
                Ok(request) => Some(request),
                Err(_) => return,
            }
        } else {
            match rx.try_recv() {
                Ok(request) => Some(request),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };

        match request {
            Some(Request::Open {
                generation,
                render_generation,
                path,
                password,
            }) => {
                doc_gen = generation;
                render_gen = render_generation;
                transform = ViewTransform::default();
                crop_job = None;
                renders = Queue::default();
                thumbs = Queue::default();
                search = None;
                last_search = None;
                select = None;
                select_line = None;
                link = None;
                article = None;
                char_cache.clear();
                labels_due = false;
                let stamp = modified(&path);
                if let Some(i) = documents.iter().position(|d| d.path == path) {
                    match documents.remove(i) {
                        Some(open) if open.modified == stamp => documents.push_front(open),
                        _ => {}
                    }
                }
                if documents.front().is_some_and(|d| d.path == path) {
                    if let Some(open) = documents.front() {
                        labels_due = true;
                        on_event(Event::Opened {
                            generation: doc_gen,
                            title: file_title(&path),
                            path,
                            page_sizes: open.page_sizes.clone(),
                            outline: open.outline.clone(),
                        });
                    }
                    continue;
                }
                match open(&pdfium, &path, password.as_deref()) {
                    Ok((doc, page_sizes)) => {
                        let outline = outline(&doc);
                        labels_due = true;
                        documents.push_front(OpenDoc {
                            path: path.clone(),
                            password: password.clone(),
                            labels: None,
                            detected_crop: None,
                            recovery: Recovering::Unloaded,
                            modified: stamp,
                            doc,
                            texts: vec![None; page_sizes.len()],
                            page_sizes: page_sizes.clone(),
                            outline: outline.clone(),
                        });
                        documents.truncate(OPEN_DOCUMENTS);
                        on_event(Event::Opened {
                            generation: doc_gen,
                            title: file_title(&path),
                            path,
                            page_sizes,
                            outline,
                        });
                    }
                    Err(PdfiumError::PdfiumLibraryInternalError(
                        PdfiumInternalError::PasswordError,
                    )) => on_event(Event::PasswordNeeded {
                        generation: doc_gen,
                        path,
                        retry: password.is_some(),
                    }),
                    Err(err) => on_event(Event::OpenFailed {
                        generation: doc_gen,
                        path,
                        message: format!("Couldn't open file: {err}"),
                    }),
                }
                continue;
            }
            Some(Request::Render {
                previews,
                pages,
                width_px,
            }) => {
                renders = Queue {
                    previews: previews.into(),
                    pages: pages.into(),
                    width_px,
                };
                continue;
            }
            Some(Request::Thumbnails { pages, width_px }) => {
                thumbs = Queue {
                    previews: VecDeque::new(),
                    pages: pages.into(),
                    width_px,
                };
                continue;
            }
            Some(Request::Select { from, to }) => {
                select_line = None;
                select = Some((from, to));
                continue;
            }
            Some(Request::SelectLine { page, point }) => {
                select = None;
                select_line = Some((page, point));
                continue;
            }
            Some(Request::LinkAt { page, point }) => {
                link = Some((page, point));
                continue;
            }
            Some(Request::SetTint {
                tint: value,
                generation,
            }) => {
                tint = value;
                render_gen = generation;
                continue;
            }
            Some(Request::SetView {
                transform: value,
                generation,
            }) => {
                transform = value;
                render_gen = generation;
                renders = Queue::default();
                thumbs = Queue::default();
                continue;
            }
            Some(Request::DetectCrop { generation, page }) => {
                crop_job = Some((generation, page));
                continue;
            }
            Some(Request::DropDocument { path }) => {
                documents.retain(|d| d.path != path);
                continue;
            }
            Some(Request::SearchFiles { id, query, paths }) => {
                let query: Vec<char> = query.trim().chars().map(fold).collect();
                files = (!query.is_empty()).then(|| FilesJob {
                    id,
                    query,
                    paths: paths.into(),
                    current: None,
                    hits: 0,
                });
                continue;
            }
            Some(Request::Typeset {
                article,
                source,
                out,
            }) => {
                // An update replaces the file: forget the open copy.
                documents.retain(|d| d.path != out);
                let result = crate::typeset::write(&pdfium, &article, &source, &out)
                    .map(|_| ())
                    .map_err(|err| err.to_string());
                on_event(Event::Typeset { path: out, result });
                continue;
            }
            Some(Request::Covers { paths, width_px }) => {
                covers = (paths.into(), width_px);
                continue;
            }
            Some(Request::Relocate { path, items }) => {
                let found = match open(&pdfium, &path, None) {
                    Ok((doc, sizes)) => relocate(&doc, sizes.len(), &items),
                    Err(_) => Vec::new(),
                };
                on_event(Event::Relocated { path, found });
                continue;
            }
            Some(Request::FindArticle { number }) => {
                article = Some(ArticleJob { number, next: 0 });
                continue;
            }
            Some(Request::Search { id, query, from }) => {
                let previous = search.take().or(last_search.take());
                let count = documents.front().map_or(0, |d| d.page_sizes.len());
                search =
                    (!query.is_empty()).then(|| SearchJob::new(id, query, count, from, previous));
                continue;
            }
            None => {}
        }

        // Other documents are searched only when the live one has nothing
        // urgent to do, so reading never waits for them.
        if !urgent {
            if let Some(path) = covers.0.pop_front() {
                let width_px = covers.1;
                let stamp_ns = modified_nanos(&path);
                let cache_path = cover_cache_path(&path);
                if let (Some(cp), Some(stamp)) = (cache_path.as_deref(), stamp_ns) {
                    if let Some((bitmap, pages)) = read_cached_cover(cp, stamp, width_px) {
                        on_event(Event::Cover {
                            path,
                            bitmap: convert(bitmap),
                            pages,
                        });
                        continue;
                    }
                }
                // Untinted: a cover shows the document as it is.
                let cover = open(&pdfium, &path, None).and_then(|(doc, sizes)| {
                    render(&doc, 0, width_px, PageTint::None, false).map(|b| (b, sizes.len()))
                });
                if let Ok((bitmap, pages)) = cover {
                    if let (Some(cp), Some(stamp)) = (cache_path.as_deref(), stamp_ns) {
                        write_cached_cover(cp, stamp, width_px, &bitmap, pages);
                    }
                    on_event(Event::Cover {
                        path,
                        bitmap: convert(bitmap),
                        pages,
                    });
                }
                continue;
            }
            if let Some(job) = &mut files {
                if search_files_step(&pdfium, job, &mut file_texts, &on_event) {
                    on_event(Event::FilesDone { id: job.id });
                    files = None;
                }
                continue;
            }
        }

        let Some(OpenDoc {
            doc,
            texts,
            labels,
            detected_crop,
            path,
            password,
            page_sizes,
            recovery,
            ..
        }) = documents.front_mut()
        else {
            continue;
        };
        let doc = &*doc;
        let mut recover = |page: usize| recovery.glyphs(path, password.as_deref(), page);

        if let Some((page, point)) = link.take() {
            let target = link_at(doc, page, point);
            on_event(Event::Link {
                generation: doc_gen,
                target,
            });
        } else if let Some((crop_gen, page)) = crop_job.take() {
            let crop = match *detected_crop {
                Some(cached) => cached,
                None => {
                    let estimated = crate::render::detect_crop(doc, page_sizes.len(), page);
                    *detected_crop = Some(estimated);
                    estimated
                }
            };
            on_event(Event::CropDetected {
                generation: crop_gen,
                crop,
            });
        } else if let Some((page, point)) = select_line.take() {
            let boxes = cached_chars(doc, &mut char_cache, page, &mut recover);
            let parts = nearest_char(&boxes, point)
                .and_then(|i| {
                    let (a, b) = line_range(&boxes, i);
                    select_range(&boxes, a, b)
                })
                .map(|(rects, text)| SelectionPart {
                    page,
                    rects,
                    text,
                    line: 0,
                })
                .into_iter()
                .collect();
            on_event(Event::Selection {
                generation: doc_gen,
                parts: number_lines(parts, |page| {
                    cached_chars(doc, &mut char_cache, page, &mut recover)
                }),
            });
        } else if let Some((from, to)) = select.take() {
            let parts = select_pages(from, to, |page| {
                cached_chars(doc, &mut char_cache, page, &mut recover)
            });
            on_event(Event::Selection {
                generation: doc_gen,
                parts: number_lines(parts, |page| {
                    cached_chars(doc, &mut char_cache, page, &mut recover)
                }),
            });
        } else if let Some(job) = &mut article {
            let page = job.next;
            job.next += 1;
            let found = page_text(doc, page)
                .is_some_and(|text| domain::nav::starts_article(&text, job.number));
            if found || job.next >= page_sizes.len() {
                on_event(Event::Article {
                    generation: doc_gen,
                    number: job.number,
                    page: found.then_some(page),
                });
                article = None;
            }
        } else if let Some(page) = renders.previews.pop_front() {
            let width_px = (renders.width_px / PREVIEW_DIVISOR).max(1);
            // The full render follows; a failed preview needs no message.
            if let Ok(bitmap) = render_transformed(doc, page, width_px, tint, false, transform) {
                on_event(Event::Rendered {
                    generation: render_gen,
                    page,
                    width_px,
                    bitmap: convert(bitmap),
                });
            }
        } else if let Some(page) = renders.pages.pop_front() {
            let width_px = renders.width_px;
            match render_transformed(doc, page, width_px, tint, true, transform) {
                Ok(bitmap) => on_event(Event::Rendered {
                    generation: render_gen,
                    page,
                    width_px,
                    bitmap: convert(bitmap),
                }),
                Err(err) => on_event(Event::Error(format!(
                    "Couldn't render page {}: {err}",
                    page + 1
                ))),
            }
        } else if let Some(job) = &mut search {
            if let Some(page) = job.next_page() {
                let text = texts.get_mut(page);
                if let Ok(matches) = search_page(doc, page, &job.query, text, &mut recover) {
                    if !matches.is_empty() {
                        job.hits.push(page);
                        on_event(Event::SearchHits {
                            id: job.id,
                            page,
                            matches,
                        });
                    }
                }
            }
            if job.is_done() {
                on_event(Event::SearchDone { id: job.id });
                last_search = search.take();
            }
        } else if let Some(page) = thumbs.pages.pop_front() {
            let width_px = thumbs.width_px;
            // A missing thumbnail isn't worth interrupting the reader for.
            if let Ok(bitmap) = render_transformed(doc, page, width_px, tint, false, transform) {
                on_event(Event::Thumbnail {
                    generation: render_gen,
                    page,
                    bitmap: convert(bitmap),
                });
            }
        } else if labels_due {
            labels_due = false;
            let found = labels.get_or_insert_with(|| {
                page_labels(&pdfium, path, password.as_deref(), page_sizes.len())
            });
            // Most documents have no labels; don't bother the UI then.
            if found.iter().any(|s| !s.is_empty()) {
                on_event(Event::Labels {
                    generation: doc_gen,
                    labels: found.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_cache_round_trips_and_validates_mtime_and_width() {
        let dir = std::env::temp_dir().join(format!("leme-cover-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sample.cover");
        let bmp = Bitmap {
            width: 2,
            height: 2,
            rgb: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        };
        write_cached_cover(&path, 12345, 220, &bmp, 42);
        let (loaded, pages) = read_cached_cover(&path, 12345, 220).expect("hit");
        assert_eq!(pages, 42);
        assert_eq!((loaded.width, loaded.height), (2, 2));
        assert_eq!(loaded.rgb, bmp.rgb);
        assert!(read_cached_cover(&path, 99999, 220).is_none());
        assert!(read_cached_cover(&path, 12345, 300).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
