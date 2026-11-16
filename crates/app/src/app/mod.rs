//! Application state above the open document: tabs and the session,
//! keybindings, the command palette, settings, themes, and overlays.

mod command_palette;
mod events;
mod keys;
mod libraries;
mod pages;
mod session_search;
mod sessions;
mod settings;
mod study_time;
mod updates;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use crate::annotations::Annotations;
use crate::config::{
    self, built_in_themes, load_themes, Config, Corners, DefaultZoom, Density, PageColors,
    ThemeDef, ThumbnailSize,
};
use crate::export::Format;
use crate::html::Article;
use crate::i18n::{self, tr, trf, untr};
use crate::keymap::{Action, Chord, KeyResult, Keymap};
use crate::library::{self, Libraries};
use crate::palette::{self, Entry, LibraryAction, Target};
use crate::pdf::{Event, PageTint, Worker};
use crate::store::{unique_name, Session, Store, DEFAULT_SESSION};
use crate::study::{self, Clock, StudyLog};
use crate::titlebar;
use crate::update;
use crate::viewer::PageBuffer;
use crate::viewer::{file_name, to_color, ui_int, PopupOutcome, SidePanel, Viewer};
use crate::web;
use crate::{
    AppWindow, BookCard, HighlightRow, KeyRow, LibraryItem, LibraryRow, PaletteItem, RecentItem,
    SearchHit, SettingsWindow, SubjectItem, TabItem, Theme, ThemeCard,
};
const PALETTE_LIMIT: usize = 12;
/// Entries of one kind (themes, commands, tabs) the palette lists at once.
const PALETTE_GROUP_LIMIT: usize = 5;
const PALETTE_PLACEHOLDER: &str = "Search commands, tabs, files, themes, or type a page number…";
const ADD_PLACEHOLDER: &str = "Type a path or search recent files…";
/// Shades offered for highlight colors in Settings; any hex works too.
const HIGHLIGHT_PRESETS: [&str; 10] = [
    "#d9a441", "#d4664d", "#68a48d", "#6f93ba", "#c27696", "#d98a2b", "#9a86c4", "#5fa3a8",
    "#a3a857", "#a39680",
];
/// Session search hits listed; more than a reader would step through.
const MAX_SESSION_HITS: usize = 400;
/// What the house themes are, shown under their previews in Settings.
const THEME_TAGLINES: [(&str, &str); 3] = [
    ("Noite", "by candlelight"),
    ("Velino", "paper by day"),
    ("Maré", "overcast sea"),
];
/// Actions whose shortcuts the window shows next to its controls, in the
/// order of its `header-keys`.
const HINT_KEYS: [Action; 15] = [
    Action::FitWidth,
    Action::FitPage,
    Action::ZoomOut,
    Action::ZoomIn,
    Action::ZoomReset,
    Action::CommandPalette,
    Action::MarkSection,
    Action::ExportNotes,
    Action::AddNote,
    Action::Copy,
    Action::Search,
    Action::SearchSession,
    Action::Open,
    Action::ToggleSidebar,
    Action::Settings,
];
/// Characters a session search needs before it starts.
const MIN_SESSION_QUERY: usize = 3;
/// Lines of the export shown in the export sheet.
const EXPORT_PREVIEW_LINES: usize = 14;

#[derive(Clone)]
enum Prompt {
    NewSession,
    RenameSession,
    NewLibrary,
    RenameLibrary(usize),
    /// The password of an encrypted document.
    Password(PathBuf),
}

pub struct App {
    viewer: Viewer,
    settings: SettingsWindow,
    /// Title bar caption, text, and border colors, and whether it's dark.
    title_colors: Option<(slint::Color, slint::Color, slint::Color, bool)>,
    config: Config,
    themes: Vec<ThemeDef>,
    keymap: Keymap,
    /// Problems with config.toml, themes, or keybindings, shown in Settings.
    problems: Vec<String>,

    tabs: Vec<PathBuf>,
    active: Option<usize>,
    tab_items: Rc<VecModel<TabItem>>,
    session_items: Rc<VecModel<RecentItem>>,
    /// What the name prompt is asking for.
    prompt: Option<Prompt>,
    /// A newer release, while its prompt is open or it downloads.
    update: Option<update::Release>,

    study: StudyLog,
    clock: Clock,
    /// Saves study time a little after it changes.
    study_save: slint::Timer,
    /// The study time shown in the header, to skip redundant updates.
    study_shown: String,

    export_format: Format,
    /// Group Markdown exports by outline section.
    export_by_section: bool,

    /// The search panel is searching every tab of the session, with this id.
    session_search: Option<u64>,
    next_session_search: u64,
    /// Pages found by the session search: document, page, snippet, and
    /// how many matches the page has.
    session_hits: Vec<(PathBuf, usize, String, usize)>,
    /// The hit last opened.
    session_selected: Option<usize>,
    /// The hit behind each row of the list; headings have none.
    hit_rows: Vec<Option<usize>>,
    search_hits: Rc<VecModel<SearchHit>>,
    /// Waits for a pause in typing before searching the session.
    session_timer: slint::Timer,
    /// A page to show once its document opens, and the query to highlight.
    open_at: Option<(PathBuf, usize, String)>,

    palette_entries: Vec<Entry>,
    /// The palette is the "Add to session" panel: no page numbers, and a
    /// typed path is offered.
    adding: bool,
    palette_shown: Vec<Entry>,
    palette_selected: usize,
    palette_items: Rc<VecModel<PaletteItem>>,

    libraries: Libraries,
    /// The library shown in place of the home screen, and its search.
    library_view: Option<usize>,
    library_query: String,
    /// The documents behind the library's cards, in order.
    library_books: Vec<PathBuf>,
    /// The add panel is adding to this library instead of the session.
    adding_to: Option<usize>,
    /// A page being downloaded into a library: its address and the library.
    page_for_library: Option<(String, usize)>,
    /// First pages of library documents, and their page counts.
    covers: HashMap<PathBuf, (slint::Image, usize)>,
    /// An article to show once its law opens: the law's PDF or address.
    pending_article: Option<(Option<PathBuf>, Option<String>, u32)>,
}

impl App {
    pub fn new(window: &AppWindow, settings: SettingsWindow, worker: Worker) -> Self {
        let store = Store::load();
        let tab_items = Rc::new(VecModel::default());
        let palette_items = Rc::new(VecModel::default());
        let session_items = Rc::new(VecModel::default());
        window.set_tabs(tab_items.clone().into());
        window.set_sessions(session_items.clone().into());
        window.set_palette_items(palette_items.clone().into());
        let search_hits = Rc::new(VecModel::default());
        window.set_search_hits(search_hits.clone().into());
        settings.set_version(env!("CARGO_PKG_VERSION").into());
        settings.set_data_dir(
            config::data_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
                .into(),
        );

        let mut app = Self {
            viewer: Viewer::new(window, worker, store),
            settings,
            title_colors: None,
            config: Config::default(),
            themes: built_in_themes(),
            keymap: Keymap::default(),
            problems: Vec::new(),
            tabs: Vec::new(),
            active: None,
            tab_items,
            session_items,
            prompt: None,
            update: None,
            study: StudyLog::load(),
            clock: Clock::default(),
            study_save: slint::Timer::default(),
            study_shown: String::new(),
            export_format: Format::Markdown,
            export_by_section: true,
            session_search: None,
            next_session_search: 0,
            session_hits: Vec::new(),
            session_selected: None,
            hit_rows: Vec::new(),
            search_hits,
            session_timer: slint::Timer::default(),
            open_at: None,
            palette_entries: Vec::new(),
            adding: false,
            palette_shown: Vec::new(),
            palette_selected: 0,
            palette_items,
            libraries: Libraries::default(),
            pending_article: None,
            library_view: None,
            library_query: String::new(),
            library_books: Vec::new(),
            adding_to: None,
            page_for_library: None,
            covers: HashMap::new(),
        };
        if let Some(path) = libraries_path() {
            let (libraries, problem) = Libraries::load(&path);
            app.libraries = libraries;
            app.problems.extend(problem);
        }
        app.update_libraries(window);
        app.reload_config(window);
        window.set_sidebar_visible(app.config.sidebar);

        // Attach to the last session, like `tmux attach`.
        app.load_session(window, app.config.restore_session);
        app
    }
}

/// Words each settings section answers to, for the settings search box:
/// general, reading, theme, appearance, keybindings, files.
const SECTION_WORDS: [&str; 7] = [
    "general session sessions restore startup tabs attach language idioma",
    "reading zoom fit width page default sidebar thumbnails vim keyboard highlights colors names markers study timer",
    "theme themes colors dark light custom",
    "appearance interface font text size density spacing compact corners rounded thumbnails page colors paper inverted night dark",
    "keybindings keys shortcuts bindings vim",
    "configuration config files toml data folder reload",
    "libraries library collections documents laws leis icon picture bibliotecas",
];

fn matching_sections(query: &str) -> [bool; 7] {
    let query = query.trim();
    // Every action is listed under Keybindings, so match those too.
    let action_match = Action::ALL.iter().any(|a| {
        palette::score(query, a.title()).is_some()
            || palette::score(query, &tr(a.title())).is_some()
    });
    let mut shown = SECTION_WORDS.map(|words| {
        query.is_empty() || words.split(' ').any(|w| palette::score(query, w).is_some())
    });
    shown[4] |= action_match;
    shown
}

/// The Settings label for a config choice, e.g. `Density::Compact` -> "Compact".
fn label(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

fn session_detail(session: &Session, current: bool) -> String {
    let tabs = match session.tabs.len() {
        1 => tr("1 tab"),
        n => trf("{} tabs", &[&n]),
    };
    if current {
        trf("{} · current", &[&tabs])
    } else {
        tabs
    }
}

/// A binding as the interface shows it: "Ctrl+Shift+F" -> "Ctrl Shift F".
/// The plus key itself stays: "Ctrl++" -> "Ctrl +".
fn key_label(keys: &str) -> String {
    keys.replace("++", "+\u{1}")
        .replace('+', " ")
        .replace('\u{1}', "+")
}

fn apply_theme(t: &Theme<'_>, theme: &ThemeDef) {
    t.set_background(to_color(&theme.background));
    t.set_surface(to_color(&theme.surface));
    t.set_raised(to_color(&theme.raised));
    t.set_border(to_color(&theme.border));
    t.set_text(to_color(&theme.text));
    t.set_text_muted(to_color(&theme.text_muted));
    t.set_accent(to_color(&theme.accent));
    t.set_page(to_color(&theme.page));
    t.set_dark(theme.dark);
    t.set_hover(to_color(&theme.hover));
    t.set_selection(to_color(&theme.selection));
    t.set_match(to_color(&theme.match_));
    t.set_match_current(to_color(&theme.match_current));
}

/// Paints the title bars once the native windows exist, retrying briefly
/// because they appear a moment after `show()`.
pub fn paint_title_bars_soon(window: slint::Weak<AppWindow>) {
    fn attempt(window: slint::Weak<AppWindow>, tries_left: u32) {
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            let mut done = true;
            if let Some(win) = window.upgrade() {
                crate::with_app(|app| done = app.paint_title_bars(&win));
            }
            if !done && tries_left > 0 {
                attempt(window, tries_left - 1);
            }
        });
    }
    attempt(window, 40);
}

/// Opens a file with its default app, or a folder in Explorer.
fn reveal(path: &Path) {
    // Explorer returns a non-zero code even on success; nothing to check.
    let _ = Command::new("explorer.exe").arg(path).spawn();
}

/// Gives users a starting point when they first open the themes folder.
fn ensure_theme_template(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let empty = std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_none());
    if empty {
        let template = include_str!("../../../../themes/noite.toml")
            .replace("name = \"Noite\"", "name = \"My Theme\"");
        let _ = std::fs::write(dir.join("my-theme.toml"), template);
    }
}

fn libraries_path() -> Option<PathBuf> {
    config::data_dir().map(|d| d.join("libraries.toml"))
}

fn document_count(n: usize) -> String {
    if n == 1 {
        tr("1 document")
    } else {
        trf("{} documents", &[&n])
    }
}

/// English names of the library icons, for the palette.
fn icon_title(icon: &str) -> &'static str {
    match icon {
        "scale" => "Scales",
        "bookmark" => "Bookmark",
        "star" => "Star",
        "folder" => "Folder",
        "code" => "Code",
        "flask" => "Flask",
        "globe" => "Globe",
        "pencil" => "Pencil",
        "heart" => "Heart",
        _ => "Book",
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// Clears transient status messages ("Copied") after a moment.
fn clear_status_later(window: &AppWindow) {
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
        if let Some(window) = weak.upgrade() {
            window.set_status("".into());
        }
    });
}

/// `title` with the characters Windows forbids in file names replaced.
fn safe_file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_end_matches('.').to_owned();
    if name.is_empty() {
        "notes".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_file_names_are_safe() {
        assert_eq!(safe_file_name("a: b/c?"), "a- b-c-");
        assert_eq!(safe_file_name(" ... "), "notes");
    }
}
