//! Libraries, their screen and settings, and law shortcuts.

use super::*;

impl App {
    /// Documents in libraries, for the palettes; tabs already open skip.
    pub(super) fn library_entries(&self) -> Vec<Entry> {
        let mut seen: Vec<&PathBuf> = Vec::new();
        let mut entries = Vec::new();
        for (library, doc) in self.libraries.documents() {
            if seen.contains(&doc) || self.tabs.iter().any(|t| same_file(t, doc)) {
                continue;
            }
            seen.push(doc);
            let names: Vec<&str> = self
                .libraries
                .holding(doc)
                .iter()
                .filter_map(|&i| self.libraries.items.get(i).map(|l| l.name.as_str()))
                .collect();
            entries.push(Entry {
                target: Target::LibraryDoc(doc.clone()),
                title: file_name(doc),
                detail: if names.is_empty() {
                    library.name.clone()
                } else {
                    names.join(", ")
                },
                keys: String::new(),
            });
        }
        entries
    }

    /// Shows a library in place of the home screen.
    pub fn open_library(&mut self, window: &AppWindow, index: usize) {
        if index >= self.libraries.items.len() {
            return;
        }
        if self.active.is_some() {
            self.go_home(window);
        }
        self.library_view = Some(index);
        self.library_query.clear();
        window.set_library_query("".into());
        window.set_library_view(true);
        self.update_library_view(window);
        let missing: Vec<PathBuf> = self.libraries.items[index]
            .documents
            .iter()
            .filter(|d| !self.covers.contains_key(*d) && d.is_file())
            .cloned()
            .collect();
        if !missing.is_empty() {
            self.viewer.request_covers(missing);
        }
    }

    pub(super) fn close_library(&mut self, window: &AppWindow) {
        if self.library_view.take().is_some() {
            window.set_library_view(false);
            self.library_books.clear();
        }
    }

    /// A button or card on the library screen.
    pub fn library_screen_action(&mut self, window: &AppWindow, action: &str, index: usize) {
        let Some(library) = self.library_view else {
            return;
        };
        match action {
            "back" => self.close_library(window),
            "open" => {
                if let Some(doc) = self.library_books.get(index).cloned() {
                    self.close_library(window);
                    self.open_path(window, doc);
                }
            }
            // The add panel, adding to this library.
            "add" => {
                self.open_add_panel(window);
                self.adding_to = Some(library);
                let name = self
                    .libraries
                    .items
                    .get(library)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                window.set_palette_placeholder(
                    trf(
                        "Add to {}: a file, a recent document, or a page's address…",
                        &[&name],
                    )
                    .into(),
                );
            }
            "manage" => self.library_palette(window, library),
            _ => {}
        }
    }

    /// What was picked in the add panel while it adds to `library`:
    /// files and documents go in at once, pages once they're saved.
    pub(super) fn add_to_library(&mut self, window: &AppWindow, library: usize, target: Target) {
        let docs = match target {
            Target::ChooseFile => rfd::FileDialog::new()
                .set_title(tr("Add PDFs"))
                .add_filter("PDF", &["pdf"])
                .pick_files()
                .unwrap_or_default(),
            Target::Recent(path) | Target::LibraryDoc(path) => vec![path],
            Target::Law {
                path: Some(path), ..
            } => vec![path],
            Target::Law { url: Some(url), .. } | Target::Page(url) => {
                self.page_for_library = Some((url.clone(), library));
                self.flash(window, &tr("Downloading page…"));
                let source = web::Source {
                    url,
                    revoked: self.config.show_revoked,
                };
                crate::load_page(window, source, None);
                return;
            }
            _ => return,
        };
        for doc in &docs {
            self.libraries.add(library, doc);
        }
        if !docs.is_empty() {
            self.libraries_changed(window);
            self.viewer.request_covers(docs);
        }
    }

    pub fn library_searched(&mut self, window: &AppWindow, query: &str) {
        self.library_query = query.to_owned();
        self.update_library_view(window);
    }

    /// A document's first page arrived: show it on its card.
    pub(super) fn cover_rendered(
        &mut self,
        window: &AppWindow,
        path: PathBuf,
        bitmap: PageBuffer,
        pages: usize,
    ) {
        let shown = self.library_books.contains(&path);
        self.covers
            .insert(path, (slint::Image::from_rgb8(bitmap), pages));
        if shown {
            self.update_library_view(window);
        }
    }

    pub(super) fn update_library_view(&mut self, window: &AppWindow) {
        let Some(library) = self.library_view.and_then(|i| self.libraries.items.get(i)) else {
            self.close_library(window);
            return;
        };
        let query = self.library_query.trim();
        let mut books: Vec<(PathBuf, i32)> = library
            .documents
            .iter()
            .filter_map(|d| palette::score(query, &file_name(d)).map(|s| (d.clone(), s)))
            .collect();
        if !query.is_empty() {
            books.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        }
        self.library_books = books.into_iter().map(|(d, _)| d).collect();

        let cards: Vec<BookCard> = self
            .library_books
            .iter()
            .map(|doc| {
                let cover = self.covers.get(doc);
                let page = self.viewer.store.recent_for(doc).map(|r| r.page);
                let pages = cover.map(|c| c.1);
                let (detail, progress) = match (page, pages) {
                    (Some(page), Some(pages)) if pages > 0 => (
                        trf("page {} of {}", &[&(page + 1), &pages]),
                        Some((page + 1) as f32 / pages as f32),
                    ),
                    (Some(page), _) => (trf("page {}", &[&(page + 1)]), None),
                    (None, _) if !doc.is_file() => (tr("File not found"), None),
                    (None, _) => (tr("Not started"), None),
                };
                BookCard {
                    title: file_name(doc).into(),
                    detail: detail.into(),
                    percent: progress
                        .map(|p| format!("{}%", (p * 100.0).round()))
                        .unwrap_or_default()
                        .into(),
                    progress: progress.unwrap_or(0.0),
                    has_progress: progress.is_some(),
                    has_cover: cover.is_some(),
                    cover: cover.map(|c| c.0.clone()).unwrap_or_default(),
                }
            })
            .collect();
        // Continue with the library's most recently read document.
        let hero = self
            .viewer
            .store
            .recent
            .iter()
            .find_map(|r| self.library_books.iter().position(|d| *d == r.path))
            .map_or(-1, ui_int);

        let image = library
            .image
            .as_deref()
            .and_then(|p| slint::Image::load_from_path(p).ok());
        let study: u64 = library
            .documents
            .iter()
            .map(|d| self.study.for_path(d))
            .sum();
        let mut summary = vec![document_count(library.documents.len())];
        if self.config.study_timer && study >= 60 {
            summary.push(trf("{} studied", &[&study::format_duration(study)]));
        }
        window.set_library_name(library.name.as_str().into());
        window.set_library_icon(library.icon.as_str().into());
        window.set_library_has_image(image.is_some());
        window.set_library_image(image.unwrap_or_default());
        window.set_library_summary(summary.join(" · ").into());
        window.set_library_books(Rc::new(VecModel::from(cards)).into());
        window.set_library_hero(hero);
    }

    /// Laws matching a shortcut typed in the palette: "cf 37" (a shortcut
    /// and maybe an article) or "8112" (a saved law's number).
    pub(super) fn law_entries(&self, query: &str) -> Vec<Entry> {
        let Some(q) = library::law_query(query) else {
            return Vec::new();
        };
        let title = |name: &str| match q.article {
            Some(a) => trf("{}, art. {}", &[&name, &a]),
            None => name.to_owned(),
        };
        let pages = config::data_dir().map(|d| d.join("pages"));
        if q.is_number() {
            let dotted = library::dotted(&q.key);
            let mut docs: Vec<&PathBuf> = self.libraries.documents().map(|(_, d)| d).collect();
            docs.extend(self.viewer.store.recent.iter().map(|r| &r.path));
            docs.extend(self.tabs.iter());
            let mut seen: Vec<&PathBuf> = Vec::new();
            return docs
                .into_iter()
                .filter(|d| {
                    let name = file_name(d);
                    let words: Vec<&str> = name
                        .split(|c: char| !c.is_alphanumeric() && c != '.')
                        .collect();
                    words.iter().any(|w| *w == dotted || *w == q.key)
                })
                .filter(|d| {
                    let new = !seen.contains(d);
                    seen.push(d);
                    new
                })
                .take(3)
                .map(|d| Entry {
                    title: title(&file_name(d)),
                    detail: q.key.clone(),
                    target: Target::Law {
                        path: Some(d.clone()),
                        url: None,
                        article: q.article,
                    },
                    keys: String::new(),
                })
                .collect();
        }
        let (name, url) = match self
            .config
            .aliases
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&q.key))
        {
            Some((key, url)) => (key.to_uppercase(), web::address(url)),
            None => match library::alias(&q.key) {
                Some((name, url)) => (name.to_owned(), Some(url)),
                None => return Vec::new(),
            },
        };
        let Some(url) = url else {
            return Vec::new();
        };
        let path = pages.and_then(|p| web::saved(&p, &url));
        vec![Entry {
            title: title(&name),
            detail: if path.is_some() {
                q.key.clone()
            } else {
                tr("downloads from planalto.gov.br")
            },
            target: Target::Law {
                path,
                url: Some(url),
                article: q.article,
            },
            keys: String::new(),
        }]
    }

    /// Jumps to the article asked for, if its law is the open document.
    pub(super) fn show_pending_article(&mut self) {
        let Some((path, url, article)) = self.pending_article.clone() else {
            return;
        };
        let Some(open) = self.active() else { return };
        let same = path.as_deref() == Some(&open)
            || url.is_some_and(|u| web::source_of(&open).is_some_and(|s| s.url == u));
        if same {
            self.pending_article = None;
            self.viewer.find_article(article);
        }
    }

    /// The libraries, to open one, or (`adding`) to add the open
    /// document to one.
    pub(super) fn open_libraries(&mut self, window: &AppWindow, adding: bool) {
        let holding = self
            .active()
            .map(|d| self.libraries.holding(&d))
            .unwrap_or_default();
        let mut entries: Vec<Entry> = self
            .libraries
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| !adding || !holding.contains(i))
            .map(|(i, l)| Entry {
                target: if adding {
                    Target::AddTo(i)
                } else {
                    Target::Library(i)
                },
                title: l.name.clone(),
                detail: document_count(l.documents.len()),
                keys: String::new(),
            })
            .collect();
        entries.push(Entry {
            target: Target::Command(Action::NewLibrary),
            title: tr("New library…"),
            detail: String::new(),
            keys: self.key_hint(Action::NewLibrary),
        });
        self.show_palette(window, entries, false);
    }

    /// One library in the palette: its documents, and what can be done
    /// with it.
    pub(super) fn library_palette(&mut self, window: &AppWindow, index: usize) {
        let Some(library) = self.libraries.items.get(index) else {
            return;
        };
        let mut entries: Vec<Entry> = library
            .documents
            .iter()
            .map(|doc| Entry {
                target: Target::LibraryDoc(doc.clone()),
                title: file_name(doc),
                detail: library.name.clone(),
                keys: String::new(),
            })
            .collect();
        let action = |action, title: &str| Entry {
            target: Target::LibraryAction(index, action),
            title: trf(title, &[&library.name]),
            detail: String::new(),
            keys: String::new(),
        };
        if let Some(doc) = self.active() {
            if library.documents.contains(&doc) {
                entries.push(action(
                    LibraryAction::RemoveCurrent,
                    "Remove this document from {}",
                ));
            } else {
                entries.push(action(LibraryAction::AddCurrent, "Add this document to {}"));
            }
        }
        entries.push(action(LibraryAction::Icon, "Change the icon of {}…"));
        entries.push(action(LibraryAction::Picture, "Use a picture for {}…"));
        entries.push(action(LibraryAction::Rename, "Rename {}…"));
        entries.push(action(LibraryAction::Delete, "Delete library {}"));
        self.show_palette(window, entries, false);
    }

    pub(super) fn library_action(&mut self, window: &AppWindow, i: usize, action: LibraryAction) {
        let Some(library) = self.libraries.items.get(i) else {
            return;
        };
        let name = library.name.clone();
        match action {
            LibraryAction::AddCurrent | LibraryAction::RemoveCurrent => {
                let Some(doc) = self.active() else { return };
                if action == LibraryAction::AddCurrent {
                    self.libraries.add(i, &doc);
                } else {
                    self.libraries.remove(i, &doc);
                }
                self.libraries_changed(window);
            }
            LibraryAction::Icon => {
                let entries = library::ICONS
                    .iter()
                    .map(|icon| Entry {
                        target: Target::LibraryIcon(i, (*icon).to_owned()),
                        title: tr(icon_title(icon)),
                        detail: if library.icon == *icon && library.image.is_none() {
                            tr("in use")
                        } else {
                            String::new()
                        },
                        keys: String::new(),
                    })
                    .collect();
                self.show_palette(window, entries, false);
            }
            LibraryAction::Picture => {
                let picked = rfd::FileDialog::new()
                    .set_title(trf("Picture for {}", &[&name]))
                    .add_filter("Image", &["png", "jpg", "jpeg", "svg", "webp"])
                    .pick_file();
                if let (Some(path), Some(library)) = (picked, self.libraries.items.get_mut(i)) {
                    library.image = Some(path);
                    self.libraries_changed(window);
                }
            }
            LibraryAction::Rename => self.ask_name(window, Prompt::RenameLibrary(i), &name),
            LibraryAction::Delete => {
                // Only the list goes; the documents stay where they are.
                self.libraries.items.remove(i);
                self.libraries_changed(window);
                self.flash(
                    window,
                    &trf("Deleted library {} (its documents are kept)", &[&name]),
                );
            }
        }
    }

    /// A change from the Libraries section of Settings. `value` is the
    /// library's index, then (after a line break) a name, an icon, or a
    /// document's index.
    pub(super) fn library_setting(&mut self, window: &AppWindow, change: &str, value: &str) {
        let (index, arg) = value.split_once('\n').unwrap_or((value, ""));
        let i: usize = index.parse().unwrap_or(usize::MAX);
        match change {
            "new" => {
                if self.libraries.create(value, library::ICONS[0]).is_none() {
                    return;
                }
            }
            "rename" => {
                if !self.libraries.rename(i, arg) {
                    self.flash(window, &tr("A library with that name already exists"));
                }
            }
            "icon" => {
                let icon = library::ICONS
                    .iter()
                    .find(|icon| **icon == arg || tr(icon_title(icon)) == arg);
                if let (Some(icon), Some(library)) = (icon, self.libraries.items.get_mut(i)) {
                    library.icon = (*icon).to_owned();
                    library.image = None;
                }
            }
            "picture" => return self.library_action(window, i, LibraryAction::Picture),
            "clear-picture" => {
                if let Some(library) = self.libraries.items.get_mut(i) {
                    library.image = None;
                }
            }
            "remove-doc" => {
                let doc = arg
                    .parse::<usize>()
                    .ok()
                    .and_then(|j| self.libraries.items.get(i)?.documents.get(j).cloned());
                if let Some(doc) = doc {
                    self.libraries.remove(i, &doc);
                }
            }
            "delete" if i < self.libraries.items.len() => {
                self.libraries.items.remove(i);
            }
            _ => return,
        }
        self.libraries_changed(window);
    }

    pub(super) fn libraries_changed(&mut self, window: &AppWindow) {
        if let Some(path) = libraries_path() {
            if let Err(err) = self.libraries.save(&path) {
                self.flash(window, &trf("Couldn't save libraries: {}", &[&err]));
            }
        }
        self.update_libraries(window);
        if self.library_view.is_some() {
            self.update_library_view(window);
        }
    }

    pub(super) fn update_libraries(&self, window: &AppWindow) {
        // Each cover image is decoded once, for the start screen and Settings.
        let (items, rows): (Vec<LibraryItem>, Vec<LibraryRow>) = self
            .libraries
            .items
            .iter()
            .map(|l| {
                let image = l
                    .image
                    .as_deref()
                    .and_then(|p| slint::Image::load_from_path(p).ok());
                let documents: Vec<slint::SharedString> =
                    l.documents.iter().map(|d| file_name(d).into()).collect();
                let item = LibraryItem {
                    name: l.name.as_str().into(),
                    icon: l.icon.as_str().into(),
                    has_image: image.is_some(),
                    image: image.clone().unwrap_or_default(),
                    count: l.documents.len().to_string().into(),
                };
                let row = LibraryRow {
                    name: item.name.clone(),
                    icon: item.icon.clone(),
                    icon_title: tr(icon_title(&l.icon)).into(),
                    has_image: image.is_some(),
                    image: image.unwrap_or_default(),
                    count: document_count(l.documents.len()).into(),
                    documents: Rc::new(VecModel::from(documents)).into(),
                };
                (item, row)
            })
            .unzip();
        window.set_libraries(Rc::new(VecModel::from(items)).into());
        self.settings
            .set_library_rows(Rc::new(VecModel::from(rows)).into());
        let icons: Vec<slint::SharedString> = library::ICONS
            .iter()
            .map(|i| tr(icon_title(i)).into())
            .collect();
        self.settings
            .set_library_icons(Rc::new(VecModel::from(icons)).into());
        let keys: Vec<slint::SharedString> = library::ICONS.iter().map(|i| (*i).into()).collect();
        self.settings
            .set_library_icon_keys(Rc::new(VecModel::from(keys)).into());
    }

    /// Saved laws go into the Laws library, made on first use.
    pub(super) fn file_law(&mut self, window: &AppWindow, pdf: &Path) {
        let law = web::source_of(pdf).is_some_and(|s| crate::html::is_planalto(&s.url));
        if !law || !self.libraries.holding(pdf).is_empty() {
            return;
        }
        if let Some(i) = self.libraries.create(library::LAWS, "scale") {
            self.libraries.add(i, pdf);
            self.libraries_changed(window);
        }
    }
}
