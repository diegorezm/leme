//! The command palette and the Add to session panel.

use super::*;

impl App {
    /// The palette as a picker of what to add to the session: a file from
    /// disk, a typed path, or a recent document from any session.
    pub(super) fn open_add_panel(&mut self, window: &AppWindow) {
        self.adding_to = None;
        let mut entries = vec![Entry {
            target: Target::ChooseFile,
            title: tr("Choose file…"),
            detail: String::new(),
            keys: self.key_hint(Action::ChooseFile),
        }];
        entries.extend(self.recent_entries());
        entries.extend(self.library_entries());
        self.show_palette(window, entries, true);
    }

    pub(super) fn recent_entries(&self) -> Vec<Entry> {
        self.viewer
            .store
            .recent
            .iter()
            .filter(|r| !self.tabs.iter().any(|t| same_file(t, &r.path)))
            .filter(|r| self.libraries.holding(&r.path).is_empty())
            .map(|r| Entry {
                target: Target::Recent(r.path.clone()),
                title: file_name(&r.path),
                detail: r
                    .path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                keys: String::new(),
            })
            .collect()
    }

    pub(super) fn show_palette(&mut self, window: &AppWindow, entries: Vec<Entry>, adding: bool) {
        self.palette_entries = entries;
        self.adding = adding;
        if !adding {
            self.adding_to = None;
        }
        let placeholder = if adding {
            ADD_PLACEHOLDER
        } else {
            PALETTE_PLACEHOLDER
        };
        window.set_palette_placeholder(tr(placeholder).into());
        window.set_palette_query("".into());
        self.palette_edited(window, "");
        self.set_overlay(window, "palette");
    }

    pub(super) fn open_palette(&mut self, window: &AppWindow) {
        let mut entries: Vec<Entry> = Vec::new();
        for (i, path) in self.tabs.iter().enumerate() {
            entries.push(Entry {
                target: Target::Tab(i),
                title: file_name(path),
                detail: tr("Open tab"),
                keys: String::new(),
            });
        }
        for action in Action::ALL {
            if *action == Action::CommandPalette || *action == Action::Escape {
                continue;
            }
            entries.push(Entry {
                target: Target::Command(*action),
                title: tr(action.title()),
                detail: String::new(),
                keys: self.key_hint(*action),
            });
        }
        entries.extend(self.session_entries());
        for theme in &self.themes {
            let current = theme.name.eq_ignore_ascii_case(&self.config.theme);
            entries.push(Entry {
                target: Target::Theme(theme.name.clone()),
                title: trf("Theme: {}", &[&theme.name]),
                detail: tr(match (current, theme.dark) {
                    (true, _) => "in use",
                    (false, true) => "Dark",
                    (false, false) => "Light",
                }),
                keys: String::new(),
            });
        }
        entries.extend(self.recent_entries());
        entries.extend(self.library_entries());
        self.show_palette(window, entries, false);
    }

    pub fn palette_edited(&mut self, window: &AppWindow, query: &str) {
        let ranked = palette::filter(&self.palette_entries, query, usize::MAX);
        let mut shown = palette::grouped(ranked, PALETTE_GROUP_LIMIT, PALETTE_LIMIT);
        shown.splice(0..0, self.law_entries(query));
        if self.adding {
            if let Some(url) = web::address(query) {
                shown.insert(
                    0,
                    Entry {
                        title: tr("Save page as document"),
                        detail: url.clone(),
                        target: Target::Page(url),
                        keys: String::new(),
                    },
                );
            }
            if let Some(path) = palette::typed_path(query).filter(|p| p.is_file()) {
                shown.insert(
                    0,
                    Entry {
                        title: file_name(&path),
                        detail: tr("Add to session"),
                        target: Target::Recent(path),
                        keys: String::new(),
                    },
                );
            }
        } else if let Some(page) = palette::page_query(query) {
            if page <= self.viewer.page_count() {
                shown.insert(
                    0,
                    Entry {
                        target: Target::GoToPage(page - 1),
                        title: trf("Go to page {}", &[&page]),
                        detail: String::new(),
                        keys: String::new(),
                    },
                );
            }
        }
        self.palette_shown = shown;
        self.palette_selected = 0;
        self.update_palette(window);
    }

    pub fn palette_moved(&mut self, window: &AppWindow, delta: i32) {
        let len = self.palette_shown.len();
        if len == 0 {
            return;
        }
        let next = (self.palette_selected as i64 + i64::from(delta)).rem_euclid(len as i64);
        self.palette_selected = next as usize;
        self.update_palette(window);
    }

    pub fn palette_accept(&mut self, window: &AppWindow, index: Option<usize>) {
        let index = index.unwrap_or(self.palette_selected);
        let Some(entry) = self.palette_shown.get(index).cloned() else {
            return;
        };
        self.set_overlay(window, "");
        if let Some(library) = self.adding_to.take() {
            return self.add_to_library(window, library, entry.target);
        }
        match entry.target {
            Target::Command(action) => self.run(window, action),
            Target::Tab(i) => self.activate(window, i),
            Target::Recent(path) => self.open_path(window, path),
            Target::GoToPage(page) => self.viewer.go_to_page(window, page),
            Target::Theme(name) => self.setting(window, "theme", &name),
            Target::Session(i) => self.switch_session(window, i),
            Target::ChooseFile => self.open_dialog(window),
            Target::Library(i) => self.open_library(window, i),
            Target::Law { path, url, article } => {
                if let Some(article) = article {
                    self.pending_article = Some((path.clone(), url.clone(), article));
                }
                match (path, url) {
                    (Some(path), _) => {
                        if self.active().as_deref() == Some(&path) {
                            self.show_pending_article();
                        } else {
                            self.open_path(window, path);
                        }
                    }
                    (None, Some(url)) => self.open_page(window, &url),
                    (None, None) => {}
                }
            }
            Target::LibraryDoc(path) => self.open_path(window, path),
            Target::AddTo(i) => {
                if let Some(doc) = self.active() {
                    self.libraries.add(i, &doc);
                    self.libraries_changed(window);
                    let name = self
                        .libraries
                        .items
                        .get(i)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    self.flash(window, &trf("Added to {}", &[&name]));
                }
            }
            Target::LibraryIcon(i, icon) => {
                if let Some(library) = self.libraries.items.get_mut(i) {
                    library.icon = icon;
                    library.image = None;
                    self.libraries_changed(window);
                }
            }
            Target::LibraryAction(i, action) => self.library_action(window, i, action),
            Target::Page(url) => {
                self.flash(window, &tr("Downloading page…"));
                let source = web::Source {
                    url,
                    revoked: self.config.show_revoked,
                };
                crate::load_page(window, source, None);
            }
        }
    }

    pub(super) fn update_palette(&self, window: &AppWindow) {
        // Each group's heading goes on its first entry.
        let mut group = None;
        let items: Vec<PaletteItem> = self
            .palette_shown
            .iter()
            .map(|e| {
                let here = e.target.group();
                let header = if group == Some(here) {
                    String::new()
                } else {
                    tr(here.title())
                };
                group = Some(here);
                let theme = match &e.target {
                    Target::Theme(name) => self.themes.iter().find(|t| t.name == *name),
                    _ => None,
                };
                let color = |pick: fn(&ThemeDef) -> &String| {
                    theme.map(|t| to_color(pick(t))).unwrap_or_default()
                };
                PaletteItem {
                    title: e.title.as_str().into(),
                    detail: e.detail.as_str().into(),
                    keys: e.keys.as_str().into(),
                    header: header.into(),
                    swatch: theme.is_some(),
                    color1: color(|t| &t.background),
                    color2: color(|t| &t.page),
                    color3: color(|t| &t.accent),
                }
            })
            .collect();
        self.palette_items.set_vec(items);
        window.set_palette_selected(ui_int(self.palette_selected));
        debug_assert!(self.palette_items.row_count() == self.palette_shown.len());
    }
}
