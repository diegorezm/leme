//! Keys: chords to actions, and running actions.

use super::*;

impl App {
    pub fn key_pressed(
        &mut self,
        window: &AppWindow,
        text: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let Some(chord) = Chord::from_event(text, ctrl, shift, alt) else {
            return false;
        };
        match self.keymap.press(chord) {
            KeyResult::Action(action) => {
                self.run(window, action);
                true
            }
            KeyResult::Pending => true,
            // Enter on the home screen continues the last document.
            KeyResult::Unbound if text == "\n" && !ctrl && !alt => self.continue_reading(window),
            KeyResult::Unbound => false,
        }
    }

    /// Opens the session's most recently read document from the home
    /// screen. Returns false when there is none, or a document is open.
    pub(super) fn continue_reading(&mut self, window: &AppWindow) -> bool {
        if self.active.is_some() || !window.get_overlay().is_empty() {
            return false;
        }
        match self.last_read_tab() {
            Some(index) => {
                self.activate(window, index);
                true
            }
            None => false,
        }
    }

    /// The session's tab that was read last, and the page it was left on.
    pub(super) fn last_read(&self) -> Option<(usize, usize)> {
        self.viewer.store.recent.iter().find_map(|recent| {
            let index = self.tabs.iter().position(|t| same_file(t, &recent.path))?;
            Some((index, recent.page))
        })
    }

    pub(super) fn last_read_tab(&self) -> Option<usize> {
        self.last_read()
            .map(|(index, _)| index)
            .or((!self.tabs.is_empty()).then_some(0))
    }

    pub fn run(&mut self, window: &AppWindow, action: Action) {
        match action {
            Action::Open => self.open_add_panel(window),
            Action::ChooseFile => self.open_dialog(window),
            Action::UpdatePage => match self.active_source() {
                Some(source) => {
                    self.flash(window, &tr("Downloading page…"));
                    crate::load_page(window, source, None);
                }
                None => self.flash(window, &tr("This document didn't come from a web page")),
            },
            Action::Libraries => self.open_libraries(window, false),
            Action::AddToLibrary if self.active.is_some() => self.open_libraries(window, true),
            Action::NewLibrary => self.ask_name(window, Prompt::NewLibrary, ""),
            Action::ToggleRevoked => {
                let saved = self.active().and_then(|p| web::saved_html(&p));
                match (self.active_source(), saved) {
                    (Some(mut source), Some(html)) => {
                        source.revoked = !source.revoked;
                        crate::load_page(window, source, Some(html));
                    }
                    _ => self.flash(window, &tr("This document didn't come from a web page")),
                }
            }
            Action::CommandPalette => self.open_palette(window),
            Action::Search if self.viewer.page_count() > 0 => self.open_search(window, false),
            Action::SearchSession => self.open_search(window, true),
            Action::GoToPage if self.viewer.page_count() > 0 => {
                window.set_goto_text(ui_int(self.viewer.current_page() + 1).to_string().into());
                self.set_overlay(window, "goto");
            }
            Action::Settings => self.toggle_settings(window),
            Action::ToggleSidebar => {
                window.set_sidebar_visible(!window.get_sidebar_visible());
                self.viewer.sidebar_shown(window);
                // Closing from the sidebar's own button removes the focused
                // element; take focus back so Ctrl+B still reaches us.
                window.invoke_focus_overlay();
            }
            Action::ShowThumbnails | Action::ShowOutline | Action::ShowNotes => {
                let panel = match action {
                    Action::ShowOutline => SidePanel::Outline,
                    Action::ShowNotes => SidePanel::Notes,
                    _ => SidePanel::Thumbs,
                };
                self.show_side_panel(window, panel, true);
            }
            Action::FocusMode => window.set_focus_mode(!window.get_focus_mode()),
            Action::Fullscreen => {
                let fullscreen = !window.get_fullscreen();
                window.set_fullscreen(fullscreen);
                window.window().set_fullscreen(fullscreen);
            }
            Action::NextTab | Action::PreviousTab => {
                let (Some(active), len) = (self.active, self.tabs.len()) else {
                    return;
                };
                let next = if action == Action::NextTab {
                    (active + 1) % len
                } else {
                    (active + len - 1) % len
                };
                self.activate(window, next);
            }
            Action::CloseTab => {
                if let Some(active) = self.active {
                    self.close_tab(window, active);
                }
            }
            Action::Home => self.go_home(window),
            Action::SwitchSession => self.open_session_picker(window),
            Action::NewSession => self.ask_name(window, Prompt::NewSession, ""),
            Action::RenameSession => {
                let name = self.session_name().to_owned();
                self.ask_name(window, Prompt::RenameSession, &name);
            }
            Action::DeleteSession => self.delete_session(window),
            Action::Copy | Action::Highlight | Action::AddNote => {
                let outcome = self.viewer.selection_action(window, action);
                self.popup_outcome(window, outcome);
            }
            Action::Color1 | Action::Color2 | Action::Color3 | Action::Color4 | Action::Color5 => {
                let index = action.color_index().unwrap_or(0);
                let outcome = self.viewer.color_key(window, index);
                self.popup_outcome(window, outcome);
            }
            Action::ToggleStudyTimer if !self.config.study_timer => {
                self.flash(window, &tr("The study timer is off in Settings"));
            }
            Action::ToggleStudyTimer => {
                self.clock.paused = !self.clock.paused;
                self.clock.reset();
                window.set_study_paused(self.clock.paused);
                let message = if self.clock.paused {
                    tr("Study timer paused")
                } else {
                    tr("Study timer running")
                };
                self.flash(window, &message);
            }
            Action::MarkSection if self.viewer.page_count() > 0 => {
                if !self.viewer.mark_current_section(window) {
                    self.flash(window, &tr("This document has no outline"));
                }
            }
            Action::Undo | Action::Redo => {
                if !self.viewer.undo(window, action == Action::Redo) {
                    let message = if action == Action::Redo {
                        tr("Nothing to redo")
                    } else {
                        tr("Nothing to undo")
                    };
                    self.flash(window, &message);
                }
            }
            Action::ExportNotes => self.export_notes(window),
            Action::CheckForUpdates => {
                window.set_status(tr("Checking for updates…").into());
                crate::check_for_update(window, true);
            }
            Action::ReloadConfig => {
                self.reload_config(window);
                window.set_status(tr("Config reloaded").into());
            }
            Action::OpenConfig => match self.config.ensure_file() {
                Some(path) => reveal(&path),
                None => window.set_status(tr("Couldn't create config.toml").into()),
            },
            Action::OpenThemesFolder => {
                if let Some(dir) = config::themes_dir() {
                    ensure_theme_template(&dir);
                    reveal(&dir);
                }
            }
            Action::Escape => self.escape(window),
            _ => {
                self.viewer.handle(window, action);
            }
        }
    }

    pub(super) fn escape(&mut self, window: &AppWindow) {
        let overlay = window.get_overlay();
        if overlay == "search" {
            self.viewer.close_search(window);
        }
        if overlay == "note" {
            self.viewer.note_cancelled();
        }
        if !overlay.is_empty() {
            self.set_overlay(window, "");
            return;
        }
        if self.library_view.is_some() {
            self.close_library(window);
            return;
        }
        if self.viewer.clear_selection(window) {
            return;
        }
        if !window.get_status().is_empty() {
            window.set_status("".into());
            return;
        }
        if window.get_fullscreen() {
            window.set_fullscreen(false);
            window.window().set_fullscreen(false);
        } else if window.get_focus_mode() {
            window.set_focus_mode(false);
        }
    }

    /// Shows the settings window, or hides it if it's already in front.
    pub fn toggle_settings(&self, window: &AppWindow) {
        let settings = self.settings.window();
        let result = if settings.is_visible() {
            settings.hide()
        } else {
            self.settings.show()
        };
        // Failing to open settings isn't worth crashing the reader over.
        let _ = result;
        paint_title_bars_soon(window.as_weak());
    }

    /// Colors both windows' title bars with the theme. Returns false if a
    /// visible window isn't ready yet.
    pub fn paint_title_bars(&self, window: &AppWindow) -> bool {
        let Some((caption, text, border, dark)) = self.title_colors else {
            return true;
        };
        let main = titlebar::paint(window.window(), caption, text, border, dark);
        let settings = !self.settings.window().is_visible()
            || titlebar::paint(self.settings.window(), caption, text, border, dark);
        main && settings
    }

    pub fn close_settings(&self) {
        let _ = self.settings.hide();
    }

    pub fn set_overlay(&mut self, window: &AppWindow, overlay: &str) {
        if overlay != "search" {
            self.end_session_search(window);
        }
        // Dismissing the password prompt gives up on that document.
        if overlay != "prompt" {
            if let Some(Prompt::Password(path)) = self.prompt.take() {
                if let Some(index) = self.tabs.iter().position(|t| *t == path) {
                    self.close_tab(window, index);
                }
            }
        }
        window.set_overlay(overlay.into());
        window.invoke_focus_overlay();
    }

    /// Shows a sidebar panel. With `toggle`, asking for the panel already
    /// shown hides the sidebar.
    pub fn show_side_panel(&mut self, window: &AppWindow, panel: SidePanel, toggle: bool) {
        let visible = window.get_sidebar_visible();
        if toggle && visible && self.viewer.side_panel() == panel {
            window.set_sidebar_visible(false);
            return;
        }
        if !visible {
            window.set_sidebar_visible(true);
        }
        self.viewer.set_side_panel(window, panel);
        self.viewer.sidebar_shown(window);
    }
}
