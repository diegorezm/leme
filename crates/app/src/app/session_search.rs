//! Search, in the document and across the session's tabs.

use super::*;

impl App {
    /// Opens the search panel, on this document or on every tab.
    pub(super) fn open_search(&mut self, window: &AppWindow, session: bool) {
        if session && self.tabs.is_empty() {
            self.flash(window, &tr("No tabs in this session"));
            return;
        }
        self.set_overlay(window, "search");
        self.set_search_scope(window, session);
    }

    /// Switches the open search between this document and every tab.
    pub fn set_search_scope(&mut self, window: &AppWindow, session: bool) {
        if !session {
            self.end_session_search(window);
            return;
        }
        if self.session_search.is_none() {
            window.set_search_scope("session".into());
            let query = window.get_search_text();
            self.session_query_edited(&query);
        }
        window.invoke_focus_overlay();
    }

    /// Back to searching the open document only; stops the worker.
    pub(super) fn end_session_search(&mut self, window: &AppWindow) {
        if self.session_search.take().is_some() {
            self.session_timer.stop();
            self.viewer.search_files(0, "", Vec::new());
            self.clear_session_hits();
        }
        window.set_search_scope("document".into());
    }

    pub(super) fn clear_session_hits(&mut self) {
        self.session_hits.clear();
        self.session_selected = None;
        self.hit_rows.clear();
        self.search_hits.set_vec(Vec::new());
    }

    /// The search box changed: highlights the open document's matches
    /// and, across tabs, lists the pages found.
    pub fn search_edited(&mut self, window: &AppWindow, query: &str) {
        if self.viewer.page_count() > 0 {
            self.viewer.search_edited(window, query);
        }
        if self.session_search.is_some() {
            self.session_query_edited(query);
        }
    }

    /// Enter in the search box: the next match, or across tabs the next
    /// page found.
    pub fn search_step(&mut self, window: &AppWindow, forward: bool) {
        let hits = self.session_hits.len();
        if self.session_search.is_none() || hits == 0 {
            return self.viewer.search_step(window, forward);
        }
        let next = match (self.session_selected, forward) {
            (None, true) => 0,
            (None, false) => hits - 1,
            (Some(i), true) => (i + 1) % hits,
            (Some(i), false) => (i + hits - 1) % hits,
        };
        self.open_hit(window, next);
    }

    /// A row of the session search list was clicked.
    pub fn search_hit_clicked(&mut self, window: &AppWindow, row: usize) {
        if let Some(Some(hit)) = self.hit_rows.get(row).copied() {
            self.open_hit(window, hit);
        }
    }

    pub(super) fn session_query_edited(&mut self, query: &str) {
        self.next_session_search += 1;
        let id = self.next_session_search;
        self.session_search = Some(id);
        self.clear_session_hits();
        let query = query.trim().to_owned();
        if query.chars().count() < MIN_SESSION_QUERY {
            self.session_timer.stop();
            self.viewer.search_files(id, "", Vec::new());
            return;
        }
        self.session_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(200),
            move || {
                crate::with_app(|app| {
                    if app.session_search == Some(id) {
                        app.viewer.search_files(id, &query, app.tabs.clone());
                    }
                });
            },
        );
    }

    pub(super) fn file_hit(
        &mut self,
        window: &AppWindow,
        id: u64,
        path: PathBuf,
        page: usize,
        snippet: String,
        count: usize,
    ) {
        if self.session_search != Some(id) || window.get_overlay() != "search" {
            return;
        }
        if self.session_hits.len() >= MAX_SESSION_HITS {
            return;
        }
        let clean = crate::viewer::unhyphenated(&snippet);
        let same_doc = self.session_hits.last().is_some_and(|(p, ..)| *p == path);
        if same_doc {
            let total: usize = self
                .session_hits
                .iter()
                .rev()
                .take_while(|(p, ..)| *p == path)
                .map(|(.., c)| *c)
                .sum::<usize>()
                + count;
            if let Some(hdr_row) = self.hit_rows.iter().rposition(Option::is_none) {
                if let Some(mut row) = self.search_hits.row_data(hdr_row) {
                    row.page = total.to_string().into();
                    self.search_hits.set_row_data(hdr_row, row);
                }
            }
        } else {
            self.search_hits.push(SearchHit {
                header: true,
                title: file_name(&path).into(),
                page: count.to_string().into(),
                current: false,
            });
            self.hit_rows.push(None);
        }
        let hit_index = self.session_hits.len();
        self.session_hits.push((path, page, clean.clone(), count));
        self.search_hits.push(SearchHit {
            header: false,
            title: clean.into(),
            page: trf("p. {}", &[&(page + 1)]).into(),
            current: false,
        });
        self.hit_rows.push(Some(hit_index));
    }

    /// Updates the selected row highlight in the session search list.
    pub(super) fn select_search_hit(&self, prev: Option<usize>, next: usize) {
        if let Some(prev) = prev {
            if let Some(row_idx) = self.hit_rows.iter().position(|r| *r == Some(prev)) {
                if let Some(mut row) = self.search_hits.row_data(row_idx) {
                    row.current = false;
                    self.search_hits.set_row_data(row_idx, row);
                }
            }
        }
        if let Some(row_idx) = self.hit_rows.iter().position(|r| *r == Some(next)) {
            if let Some(mut row) = self.search_hits.row_data(row_idx) {
                row.current = true;
                self.search_hits.set_row_data(row_idx, row);
            }
        }
    }

    /// Shows a page found by the session search, opening its tab first.
    pub(super) fn open_hit(&mut self, window: &AppWindow, hit: usize) {
        let Some((path, page, ..)) = self.session_hits.get(hit).cloned() else {
            return;
        };
        let prev = self.session_selected.replace(hit);
        self.select_search_hit(prev, hit);
        let query = window.get_search_text().trim().to_owned();
        if self.viewer.is_showing(&path) && self.viewer.page_count() > 0 {
            self.viewer.go_to_page(window, page);
        } else {
            self.open_at = Some((path.clone(), page, query));
            self.open_path(window, path);
        }
    }

    /// The document of a session hit finished opening: go to the page
    /// and highlight the matches, as if searched in the document.
    pub(super) fn show_hit(&mut self, window: &AppWindow, page: usize, query: &str) {
        self.viewer.jump_to_page(window, page);
        window.set_search_text(query.into());
        self.viewer.search_edited(window, query);
    }

    pub fn popup_action(&mut self, window: &AppWindow, action: &str, index: usize) {
        let outcome = self.viewer.popup_action(window, action, index);
        self.popup_outcome(window, outcome);
    }

    pub(super) fn popup_outcome(&mut self, window: &AppWindow, outcome: PopupOutcome) {
        match outcome {
            PopupOutcome::EditNote {
                quote,
                note,
                color,
                page,
            } => {
                window.set_note_quote(quote.into());
                window.set_note_exists(!note.trim().is_empty());
                window.set_note_text(note.into());
                window.set_note_page(trf("p. {}", &[&page]).into());
                self.show_note_color(window, color);
                self.set_overlay(window, "note");
            }
            PopupOutcome::Copied => {
                window.set_status(tr("Copied").into());
                clear_status_later(window);
            }
            PopupOutcome::Nothing => {}
        }
    }

    pub fn note_saved(&mut self, window: &AppWindow) {
        let text = window.get_note_text();
        self.viewer.note_saved(window, &text);
        self.set_overlay(window, "");
    }

    /// Removes the note being edited; its highlight stays.
    pub fn note_deleted(&mut self, window: &AppWindow) {
        self.viewer.note_saved(window, "");
        self.set_overlay(window, "");
    }

    /// A color picked in the note editor.
    pub fn note_color_clicked(&mut self, window: &AppWindow, index: usize) {
        if self.viewer.note_color(window, index) {
            self.show_note_color(window, index);
        }
    }

    pub(super) fn show_note_color(&self, window: &AppWindow, index: usize) {
        let name = crate::annotations::COLORS
            .get(index)
            .map(|(color, _)| self.config.color_name(color))
            .unwrap_or_default();
        window.set_note_color(ui_int(index));
        window.set_note_color_name(name.into());
    }

    /// Opens the export sheet for the current document.
    pub(super) fn export_notes(&mut self, window: &AppWindow) {
        if self.viewer.page_count() == 0 {
            return;
        }
        if !self.viewer.has_notes() {
            self.flash(window, &tr("No notes in this document yet"));
            return;
        }
        self.update_export(window);
        self.set_overlay(window, "export");
    }

    pub(super) fn update_export(&self, window: &AppWindow) {
        let (text, count) = self
            .viewer
            .export(self.export_format, self.export_by_section);
        let preview: Vec<&str> = text.lines().take(EXPORT_PREVIEW_LINES).collect();
        let mut preview = preview.join("\n");
        if text.lines().count() > EXPORT_PREVIEW_LINES {
            preview.push_str("\n…");
        }
        window.set_export_format(self.export_format.name().into());
        window.set_export_by_section(self.export_by_section);
        window.set_export_preview(preview.into());
        window.set_export_count(
            match count {
                1 => tr("1 note"),
                n => trf("{} notes", &[&n]),
            }
            .into(),
        );
        window.set_export_ready(count > 0);
    }

    /// A choice in the export sheet: "format" or "by-section".
    pub fn export_option(&mut self, window: &AppWindow, key: &str, value: &str) {
        match key {
            "format" => {
                if let Some(format) = Format::from_name(value) {
                    self.export_format = format;
                }
            }
            "by-section" => self.export_by_section = value == "true",
            _ => return,
        }
        self.update_export(window);
    }

    /// A color toggled in the notes filter, which the export follows.
    pub fn note_filter_clicked(&mut self, window: &AppWindow, index: Option<usize>) {
        match index {
            Some(index) => self.viewer.note_filter_clicked(index),
            None => self.viewer.note_filter_all(),
        }
        if window.get_overlay() == "export" {
            self.update_export(window);
        }
    }

    pub fn export_accepted(&mut self, window: &AppWindow) {
        let format = self.export_format;
        let (text, count) = self.viewer.export(format, self.export_by_section);
        if count == 0 {
            return;
        }
        let name = format!(
            "{}.{}",
            safe_file_name(self.viewer.title()),
            format.extension()
        );
        // A configured folder (an Obsidian vault) is written without
        // asking; re-exporting replaces the file with the current notes.
        let folder = self.config.notes_folder.trim();
        let target = if folder.is_empty() {
            rfd::FileDialog::new()
                .set_title(tr("Export notes"))
                .set_file_name(&name)
                .add_filter(format.label(), &[format.extension()])
                .save_file()
        } else {
            Some(std::path::Path::new(folder).join(&name))
        };
        let Some(path) = target else {
            return;
        };
        self.set_overlay(window, "");
        let message = match std::fs::write(&path, text) {
            Ok(()) => trf("Notes exported to {}", &[&file_name(&path)]),
            Err(err) => trf("Couldn't export notes: {}", &[&err]),
        };
        self.flash(window, &message);
    }
}
