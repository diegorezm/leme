//! Tabs and sessions.

use super::*;

impl App {
    pub(super) fn open_dialog(&mut self, window: &AppWindow) {
        // Modal native dialog; returns once the user picks or cancels.
        let picked = rfd::FileDialog::new()
            .set_title("Open PDF")
            .add_filter("PDF", &["pdf"])
            .pick_files();
        for path in picked.unwrap_or_default() {
            self.open_path(window, path);
        }
    }

    /// Opens a document in a new tab, or switches to it if already open.
    pub fn open_path(&mut self, window: &AppWindow, path: PathBuf) {
        self.close_library(window);
        let index = match self.tabs.iter().position(|t| same_file(t, &path)) {
            Some(index) => index,
            None => {
                self.tabs.push(path);
                self.tabs.len() - 1
            }
        };
        self.activate(window, index);
    }

    pub fn open_tab(&mut self, window: &AppWindow, index: usize) {
        self.activate(window, index);
    }

    pub(super) fn activate(&mut self, window: &AppWindow, index: usize) {
        let Some(path) = self.tabs.get(index).cloned() else {
            return;
        };
        let already = self.active == Some(index) && self.viewer.is_showing(&path);
        self.active = Some(index);
        if !already {
            self.viewer.remember_position();
            self.viewer.clear_selection(window);
            if !self.viewer.open(window, path) {
                self.tabs.remove(index);
                self.active = None;
                if self.tabs.is_empty() {
                    self.viewer.close(window);
                } else {
                    self.activate(window, index.min(self.tabs.len() - 1));
                    return;
                }
            }
        }
        self.tabs_changed(window);
    }

    pub fn close_tab(&mut self, window: &AppWindow, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        match self.active {
            _ if self.tabs.is_empty() => {
                self.active = None;
                self.viewer.close(window);
            }
            Some(active) if active == index => {
                self.active = None;
                self.activate(window, index.min(self.tabs.len() - 1));
            }
            Some(active) if active > index => self.active = Some(active - 1),
            _ => {}
        }
        self.tabs_changed(window);
    }

    /// Back to the home screen; the tabs stay open.
    pub fn go_home(&mut self, window: &AppWindow) {
        self.close_library(window);
        self.viewer.close(window);
        self.active = None;
        self.tabs_changed(window);
    }

    // ----- Sessions --------------------------------------------------------

    pub(super) fn session_name(&self) -> &str {
        let store = &self.viewer.store;
        store
            .sessions
            .get(store.current_session)
            .map_or(DEFAULT_SESSION, |s| s.name.as_str())
    }

    /// Shows the current session's tabs, reopening its active one if `open`.
    pub(super) fn load_session(&mut self, window: &AppWindow, open: bool) {
        let store = &self.viewer.store;
        let session = store
            .sessions
            .get(store.current_session)
            .cloned()
            .unwrap_or_else(|| Session::new(DEFAULT_SESSION));
        self.tabs = session.tabs.into_iter().filter(|p| p.exists()).collect();
        self.active = None;
        match session.active.filter(|&a| open && a < self.tabs.len()) {
            Some(active) => self.activate(window, active),
            None => {
                self.viewer.close(window);
                self.tabs_changed(window);
            }
        }
    }

    pub fn switch_session(&mut self, window: &AppWindow, index: usize) {
        let store = &self.viewer.store;
        if index == store.current_session || index >= store.sessions.len() {
            return;
        }
        self.tabs_changed(window);
        self.viewer.store.current_session = index;
        self.load_session(window, true);
        let message = trf("Session: {}", &[&self.session_name()]);
        self.flash(window, &message);
    }

    pub(super) fn new_session(&mut self, window: &AppWindow, name: &str) {
        self.tabs_changed(window);
        let store = &mut self.viewer.store;
        let name = unique_name(&store.sessions, name);
        store.sessions.push(Session::new(&name));
        store.current_session = store.sessions.len() - 1;
        self.load_session(window, false);
    }

    pub(super) fn rename_session(&mut self, window: &AppWindow, name: &str) {
        let store = &mut self.viewer.store;
        let current = store.current_session;
        let others: Vec<Session> = store
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != current)
            .map(|(_, s)| s.clone())
            .collect();
        if let Some(session) = store.sessions.get_mut(current) {
            session.name = unique_name(&others, name);
        }
        self.tabs_changed(window);
    }

    pub(super) fn delete_session(&mut self, window: &AppWindow) {
        let store = &mut self.viewer.store;
        if store.sessions.len() <= 1 {
            self.flash(window, &tr("The only session can't be deleted"));
            return;
        }
        let removed = store.sessions.remove(store.current_session);
        store.current_session = store.current_session.min(store.sessions.len() - 1);
        self.load_session(window, true);
        self.flash(window, &trf("Deleted session {}", &[&removed.name]));
    }

    pub(super) fn ask_name(&mut self, window: &AppWindow, prompt: Prompt, initial: &str) {
        window.set_prompt_label(
            tr(match prompt {
                Prompt::NewSession => "New session",
                Prompt::RenameSession => "Rename session",
                Prompt::NewLibrary => "New library",
                Prompt::RenameLibrary(_) => "Rename library",
                Prompt::Password(_) => "Password",
            })
            .into(),
        );
        window.set_prompt_secret(matches!(prompt, Prompt::Password(_)));
        self.prompt = Some(prompt);
        window.set_prompt_text(initial.into());
        self.set_overlay(window, "prompt");
    }

    pub fn prompt_accepted(&mut self, window: &AppWindow, text: &str) {
        // Taken first: closing the overlay with a password prompt pending
        // would give up on the document.
        let prompt = self.prompt.take();
        self.set_overlay(window, "");
        match prompt {
            Some(Prompt::NewSession) => self.new_session(window, text),
            Some(Prompt::RenameSession) => self.rename_session(window, text),
            Some(Prompt::NewLibrary) => {
                if let Some(i) = self.libraries.create(text, library::ICONS[0]) {
                    if let Some(doc) = self.active() {
                        self.libraries.add(i, &doc);
                    }
                    self.libraries_changed(window);
                }
            }
            Some(Prompt::RenameLibrary(i)) => {
                if self.libraries.rename(i, text) {
                    self.libraries_changed(window);
                } else {
                    self.flash(window, &tr("A library with that name already exists"));
                }
            }
            Some(Prompt::Password(path)) if self.viewer.is_pending(&path) => {
                self.viewer.open_with_password(path, text.to_owned());
            }
            Some(Prompt::Password(_)) => {}
            None => {}
        }
    }

    pub(super) fn open_session_picker(&mut self, window: &AppWindow) {
        self.palette_entries = self.session_entries();
        self.palette_entries.push(Entry {
            target: Target::Command(Action::NewSession),
            title: tr("New session…"),
            detail: String::new(),
            keys: self.key_hint(Action::NewSession),
        });
        self.adding = false;
        window.set_palette_placeholder(tr(PALETTE_PLACEHOLDER).into());
        window.set_palette_query("".into());
        self.palette_edited(window, "");
        // Start on the session after the current one, like the tmux chooser.
        let current = self.viewer.store.current_session;
        self.palette_selected = (current + 1) % self.viewer.store.sessions.len().max(1);
        self.update_palette(window);
        self.set_overlay(window, "palette");
    }

    pub(super) fn session_entries(&self) -> Vec<Entry> {
        let store = &self.viewer.store;
        store
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| Entry {
                target: Target::Session(i),
                title: trf("Session: {}", &[&s.name]),
                detail: session_detail(s, i == store.current_session),
                keys: String::new(),
            })
            .collect()
    }

    pub(super) fn flash(&self, window: &AppWindow, message: &str) {
        window.set_status(message.into());
        clear_status_later(window);
    }

    pub(super) fn tabs_changed(&mut self, window: &AppWindow) {
        let items: Vec<TabItem> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, path)| TabItem {
                title: file_name(path).into(),
                active: Some(i) == self.active,
            })
            .collect();
        self.tab_items.set_vec(items);

        let store = &mut self.viewer.store;
        let current = store.current_session;
        if let Some(session) = store.sessions.get_mut(current) {
            session.tabs = self.tabs.clone();
            session.active = self.active;
        }
        let _ = store.save();

        let sessions: Vec<RecentItem> = store
            .sessions
            .iter()
            .map(|s| RecentItem {
                title: s.name.as_str().into(),
                // The home screen marks the current one with a dot.
                folder: session_detail(s, false).into(),
            })
            .collect();
        self.session_items.set_vec(sessions);
        window.set_session_name(self.session_name().into());
        window.set_home(self.active.is_none());
        if self.active.is_none() {
            self.update_subjects(window);
        }
        self.clock.reset();
        self.show_study_time(window);
    }
}
