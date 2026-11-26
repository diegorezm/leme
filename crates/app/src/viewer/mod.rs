//! UI glue for the open document: turns layout, render results, search
//! hits, the text selection, and annotations into Slint properties, and
//! decides which pages and thumbnails to render or evict.

mod actions;
mod documents;
mod find;
mod helpers;
mod marks;
mod rendering;
mod side_panel;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel,
};

use crate::annotations::{self, Annotations, History, Kind};
use crate::cache::PageCache;
use crate::config::{parse_color, DefaultZoom};
use crate::export;
use crate::i18n::{tr, trf};
use crate::keymap::Action;
use crate::layout::{self, Layout, Zoom};
use crate::nav;
use crate::pdf::{
    Bitmap, LinkTarget, NormRect, OutlineItem, PageTint, Request, SelectionPart, Worker,
};
use crate::search::SearchState;
use crate::store::{Recent, Store};
use crate::{AppWindow, Highlight, Mark, NoteFilter, PageView, RecentItem, SideItem, ThumbView};

/// Pages rendered ahead of and behind the visible ones.
const PRELOAD: usize = 2;
/// Rendered pages kept beyond the preload window before being dropped.
const KEEP: usize = 4;
/// Thumbnails rendered around the visible ones, and kept beyond that.
const THUMB_PRELOAD: usize = 6;
const THUMB_KEEP: usize = 20;
/// Recent documents listed on the start screen.
const RECENT_SHOWN: usize = 8;
/// Line scroll step, in logical pixels.
const SCROLL_STEP: f32 = 60.0;
/// Zoom steps closer together than this count as one burst.
const ZOOM_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);
/// How long a changed reading position may wait before it is saved.
const SAVE_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Pointer travel before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 4.0;

/// The selected text, one part per page.
struct Selection {
    parts: Vec<SelectionPart>,
}

impl Selection {
    fn text(&self) -> String {
        let texts: Vec<&str> = self.parts.iter().map(|p| p.text.as_str()).collect();
        texts.join("\n")
    }
}

struct Drag {
    page: usize,
    from: (f32, f32),
    start: (f32, f32),
    dragging: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Popup {
    None,
    Selection,
    Annotation(u64),
}

/// What the app should do after a popup button.
pub enum PopupOutcome {
    Nothing,
    /// Open the note editor with this quote and existing note.
    /// `color` is the annotation's place in `COLORS`; `page` its page name.
    EditNote {
        quote: String,
        note: String,
        color: usize,
        page: String,
    },
    Copied,
}

/// What the sidebar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidePanel {
    Thumbs,
    Outline,
    Notes,
}

impl SidePanel {
    pub fn name(self) -> &'static str {
        match self {
            SidePanel::Thumbs => "thumbs",
            SidePanel::Outline => "outline",
            SidePanel::Notes => "notes",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "outline" => SidePanel::Outline,
            "notes" => SidePanel::Notes,
            _ => SidePanel::Thumbs,
        }
    }
}

/// What a document looked like when the user switched away from it, so
/// switching back shows the same spot instantly instead of re-rendering.
struct TabView {
    path: PathBuf,
    zoom: Zoom,
    viewport: (f32, f32),
    rotation: u16,
    crop_margins: bool,
    crop_box: Option<NormRect>,
    two_up: bool,
    /// Only the pages around the viewport, so this stays small.
    cache: PageCache<Image>,
}

/// Memory for rendered pages of the open document: about 18 pages of
/// 1600 px. Pages farthest from the viewport go first.
const PAGE_CACHE_BYTES: usize = 192 * 1024 * 1024;

/// Documents whose view is kept; matches the worker's open documents.
const KEPT_VIEWS: usize = 4;

pub struct Viewer {
    worker: Worker,
    pub store: Store,

    path: Option<PathBuf>,
    /// A document requested but not yet opened by the worker.
    pending: Option<PathBuf>,
    /// Bumped on every open/close to drop stale worker events.
    doc_gen: u64,
    /// Bumped on every open/close/tint change to drop stale renders.
    render_gen: u64,
    title: String,
    page_sizes: Vec<(f32, f32)>,
    layout: Layout,
    zoom: Zoom,
    default_zoom: Zoom,
    current_page: usize,
    pages: Rc<VecModel<PageView>>,
    /// Rendered pages and the pixel width they were rendered at.
    cache: PageCache<Image>,
    last_request: Option<(Range<usize>, u32)>,
    /// Pages currently in the UI model, to skip rebuilding it while scrolling.
    shown_pages: Option<Range<usize>>,
    shown_thumbs: Option<Range<usize>>,

    thumb_layout: Layout,
    /// Matches `Theme.sidebar-width`.
    sidebar_width: f32,
    thumbs: Rc<VecModel<ThumbView>>,
    thumb_cache: HashMap<usize, Image>,
    /// Pages on screen, which the page cache never evicts.
    visible: Range<usize>,
    /// Saves a changed reading position shortly after it changes.
    save_timer: slint::Timer,
    /// Runs while zoom steps follow each other quickly.
    zoom_timer: slint::Timer,
    last_thumb_request: Option<Range<usize>>,

    search: Option<SearchState>,
    next_search_id: u64,

    annotations: Annotations,
    /// True if the annotation file on disk failed to parse; avoids overwriting it.
    annotations_broken: bool,
    history: History,
    selection: Option<Selection>,
    drag: Option<Drag>,
    /// The annotation whose note tooltip is showing.
    hovered: Option<u64>,
    popup: Popup,
    /// The annotation whose note is being edited.
    note_target: Option<u64>,

    /// Views of recently left documents, most recent first.
    tab_views: Vec<TabView>,
    recent: Rc<VecModel<RecentItem>>,

    outline: Vec<OutlineItem>,
    /// Page labels, empty when the document has none.
    labels: Vec<String>,
    side_panel: SidePanel,
    side_items: Rc<VecModel<SideItem>>,
    /// The page each side item jumps to.
    side_pages: Vec<usize>,
    /// The outline entry of each side item, in the outline panel.
    side_outline: Vec<usize>,
    /// Row in `side_items` currently highlighted as active in the outline.
    current_outline_row: Option<usize>,
    /// What each highlight color means, in `annotations::COLORS` order.
    color_names: Vec<String>,
    /// The shade of each highlight color, same order.
    color_shades: Vec<Color>,
    /// Colors the notes list leaves out.
    hidden_colors: HashSet<String>,
    note_filters: Rc<VecModel<NoteFilter>>,
    /// How pages are recolored, to skip re-rendering when it didn't change.
    tint: PageTint,
    /// For the few texts set where no window is passed along.
    window: slint::Weak<AppWindow>,
    /// Set while the window is being resized: keep showing stretched
    /// pages instead of rendering every intermediate size.
    renders_held: bool,
    /// Passwords typed this run, so switching tabs doesn't ask again.
    /// Never written to disk.
    passwords: HashMap<PathBuf, String>,
    /// Jump history for `Alt+Left` / `Alt+Right` navigation.
    jumplist: nav::JumpList,
    /// Per-document clockwise rotation (0, 90, 180, 270).
    rotation: u16,
    /// Whether visual margin cropping is active for this document.
    crop_margins: bool,
    /// Estimated document-wide content region for margin cropping.
    crop_box: Option<NormRect>,
    /// Whether pages are laid out two side-by-side.
    two_up: bool,
    /// Fast file fingerprint for matching moved documents.
    doc_fingerprint: Option<String>,
    /// Disk modification time when the current document was opened/reloaded.
    loaded_modified: Option<std::time::SystemTime>,
    /// Preserved `(path, page, viewport_y)` across a reload from disk.
    reload_scroll: Option<(PathBuf, usize, f32)>,
    /// Old -> new path when a moved document matched a recent entry by fingerprint.
    migrated_path: Option<(PathBuf, PathBuf)>,
}

impl Viewer {
    pub fn new(window: &AppWindow, worker: Worker, store: Store) -> Self {
        let pages = Rc::new(VecModel::default());
        let thumbs = Rc::new(VecModel::default());
        let recent = Rc::new(VecModel::default());
        window.set_pages(pages.clone().into());
        window.set_thumbs(thumbs.clone().into());
        window.set_recent(recent.clone().into());
        let side_items = Rc::new(VecModel::default());
        window.set_side_items(side_items.clone().into());
        let note_filters = Rc::new(VecModel::default());
        window.set_note_filters(note_filters.clone().into());

        let viewer = Self {
            worker,
            store,
            path: None,
            pending: None,
            doc_gen: 0,
            render_gen: 0,
            title: String::new(),
            page_sizes: Vec::new(),
            layout: Layout::default(),
            zoom: Zoom::FitWidth,
            default_zoom: Zoom::FitWidth,
            current_page: 0,
            pages,
            cache: PageCache::new(PAGE_CACHE_BYTES),
            last_request: None,
            shown_pages: None,
            shown_thumbs: None,
            thumb_layout: Layout::default(),
            sidebar_width: 180.0,
            thumbs,
            thumb_cache: HashMap::new(),
            visible: 0..0,
            save_timer: slint::Timer::default(),
            zoom_timer: slint::Timer::default(),
            last_thumb_request: None,
            search: None,
            next_search_id: 0,
            annotations: Annotations::default(),
            annotations_broken: false,
            history: History::default(),
            selection: None,
            drag: None,
            hovered: None,
            popup: Popup::None,
            note_target: None,
            tab_views: Vec::new(),
            recent,
            outline: Vec::new(),
            labels: Vec::new(),
            side_panel: SidePanel::Thumbs,
            side_items,
            side_pages: Vec::new(),
            side_outline: Vec::new(),
            current_outline_row: None,
            color_names: annotations::COLORS
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect(),
            color_shades: annotations::COLORS
                .iter()
                .map(|(_, hex)| to_color(hex))
                .collect(),
            hidden_colors: HashSet::new(),
            note_filters,
            tint: PageTint::None,
            window: window.as_weak(),
            passwords: HashMap::new(),
            renders_held: false,
            jumplist: nav::JumpList::default(),
            rotation: 0,
            crop_margins: false,
            crop_box: None,
            two_up: false,
            doc_fingerprint: None,
            loaded_modified: None,
            reload_scroll: None,
            migrated_path: None,
        };
        viewer.update_recent();
        viewer
    }
}

fn copy_text(text: &str) -> PopupOutcome {
    #[cfg(windows)]
    let result = clipboard_win::set_clipboard_string(text).map_err(|_| ());
    #[cfg(not(windows))]
    let result = clipboard_set(text);
    match result {
        Ok(()) => PopupOutcome::Copied,
        Err(_) => PopupOutcome::Nothing,
    }
}

/// The clipboard is owned by a background thread so the text stays
/// available after this call, as X11 and Wayland require.
#[cfg(not(windows))]
fn clipboard_set(text: &str) -> Result<(), ()> {
    use std::sync::{Mutex, OnceLock};
    static CLIPBOARD: OnceLock<Option<Mutex<arboard::Clipboard>>> = OnceLock::new();
    let clipboard = CLIPBOARD
        .get_or_init(|| arboard::Clipboard::new().ok().map(Mutex::new))
        .as_ref()
        .ok_or(())?;
    let mut clipboard = clipboard.lock().map_err(|_| ())?;
    clipboard.set_text(text).map_err(|_| ())
}

fn model_or_default<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    if items.is_empty() {
        ModelRc::default()
    } else {
        Rc::new(VecModel::from(items)).into()
    }
}

/// The colors hidden after `picked` is clicked in the notes filter. With
/// nothing hidden, the click shows `picked` alone; otherwise it toggles
/// `picked`, and hiding every color in use shows them all again.
fn toggle_filter(used: &[&str], hidden: &HashSet<String>, picked: &str) -> HashSet<String> {
    let mut hidden = hidden.clone();
    if hidden.is_empty() {
        hidden = used
            .iter()
            .filter(|color| **color != picked)
            .map(|color| (*color).to_owned())
            .collect();
    } else if !hidden.remove(picked) {
        hidden.insert(picked.to_owned());
    }
    if used.iter().all(|color| hidden.contains(*color)) {
        hidden.clear();
    }
    hidden
}

/// Selected text as a quote: PDFium marks a word hyphenated across lines
/// with U+0002, which would show as a box; the word is joined back.
pub fn unhyphenated(text: &str) -> String {
    text.replace('\u{2}', "")
}

/// Shortens text to `max` characters, on a char boundary, with an ellipsis.
fn truncate(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}

pub fn to_color(hex: &str) -> Color {
    parse_color(hex).map_or(Color::default(), |[r, g, b, a]| {
        Color::from_argb_u8(a, r, g, b)
    })
}

/// Pixels ready for the UI. Pages are opaque, so RGB saves a quarter of
/// the memory of RGBA.
pub type PageBuffer = SharedPixelBuffer<Rgb8Pixel>;

/// Converts a rendered bitmap for the UI. Called on the worker thread, so
/// the UI thread never copies pixels.
pub fn to_buffer(bitmap: Bitmap) -> PageBuffer {
    PageBuffer::clone_from_slice(&bitmap.rgb, bitmap.width, bitmap.height)
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn ui_int(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_filter_solos_then_toggles() {
        let used = ["yellow", "red", "green"];
        let set = |colors: &[&str]| colors.iter().map(|c| (*c).to_owned()).collect();
        // From "all", a click shows that color alone.
        let hidden = toggle_filter(&used, &HashSet::new(), "red");
        assert_eq!(hidden, set(&["yellow", "green"]));
        // Then colors toggle.
        let hidden = toggle_filter(&used, &hidden, "green");
        assert_eq!(hidden, set(&["yellow"]));
        let hidden = toggle_filter(&used, &hidden, "green");
        assert_eq!(hidden, set(&["yellow", "green"]));
        // Hiding the last one shown brings every color back.
        assert!(toggle_filter(&used, &hidden, "red").is_empty());
        // A lone color has nothing to filter.
        assert!(toggle_filter(&["red"], &HashSet::new(), "red").is_empty());
    }
}
