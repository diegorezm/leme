//! Opening, switching, and closing documents; reading position.

use super::*;

impl Viewer {
    /// Whether `path` is open or being opened.
    pub fn is_showing(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path) || self.pending.as_deref() == Some(path)
    }

    /// Searches the text of `paths` on the worker; results arrive as
    /// `FileHit` events.
    pub fn search_files(&self, id: u64, query: &str, paths: Vec<PathBuf>) {
        self.worker.send(Request::SearchFiles {
            id,
            query: query.to_owned(),
            paths,
        });
    }

    /// Typesets a web page as a PDF on the worker; a `Typeset` event says
    /// when it's done.
    pub fn typeset(&self, article: crate::html::Article, source: String, out: PathBuf) {
        self.worker.send(Request::Typeset {
            article: Box::new(article),
            source,
            out,
        });
    }

    /// Asks the worker for the first page of each of `paths`, as covers.
    pub fn request_covers(&self, paths: Vec<PathBuf>) {
        // Twice the card width, for sharp covers on high-DPI screens.
        self.worker.send(Request::Covers {
            paths,
            width_px: 300,
        });
    }

    /// Asks the worker where the annotations of `path` now are, after the
    /// file was rewritten; `false` if it has none.
    pub fn relocate_annotations(&self, path: &Path) -> bool {
        let annotations = Annotations::load(path);
        if annotations.items.is_empty() {
            return false;
        }
        let items = annotations
            .items
            .iter()
            .map(|a| (a.id, a.text.clone(), a.page))
            .collect();
        self.worker.send(Request::Relocate {
            path: path.to_owned(),
            items,
        });
        true
    }

    pub fn find_article(&self, number: u32) {
        self.worker.send(Request::FindArticle { number });
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// Starts opening a document. Returns false if the file is gone.
    pub fn open(&mut self, window: &AppWindow, path: PathBuf) -> bool {
        if !path.exists() {
            self.store.recent.retain(|r| r.path != path);
            self.save();
            self.update_recent();
            window.set_status(trf("File not found: {}", &[&path.display()]).into());
            return false;
        }
        window.set_status("".into());
        self.keep_view(window);
        self.pending = Some(path.clone());
        let password = self.passwords.get(&path).cloned();
        self.worker.send(Request::Open { path, password });
        true
    }

    /// Retries opening an encrypted document with a password.
    pub fn open_with_password(&mut self, path: PathBuf, password: String) {
        self.passwords.insert(path.clone(), password.clone());
        self.pending = Some(path.clone());
        self.worker.send(Request::Open {
            path,
            password: Some(password),
        });
    }

    /// Whether the worker is waiting on this path, e.g. for a password.
    pub fn is_pending(&self, path: &Path) -> bool {
        self.pending.as_deref() == Some(path)
    }

    /// Back to the start screen.
    pub fn close(&mut self, window: &AppWindow) {
        self.remember_position();
        self.path = None;
        self.pending = None;
        self.title.clear();
        self.reset_document_state(window);
        self.page_sizes.clear();
        self.outline.clear();
        self.labels.clear();
        self.update_side_items();
        self.layout = Layout::default();
        self.thumb_layout = Layout::default();
        window.set_page_count(0);
        window.set_page_label("".into());
        window.set_section_title("".into());
        window.set_current_page(0);
        window.set_document_title("".into());
        self.refresh(window);
        self.update_recent();
    }

    /// Forgets what is kept of `path` because the file was rewritten, so
    /// opening it reads the new file. It reopens at its saved page.
    pub fn forget(&mut self, path: &Path) {
        self.tab_views.retain(|v| v.path != path);
        if self.path.as_deref() == Some(path) {
            self.remember_position();
            self.path = None;
        }
    }

    /// Remembers the open document's view for switching back to it.
    pub(super) fn keep_view(&mut self, window: &AppWindow) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.tab_views.retain(|v| v.path != path);
        self.tab_views.insert(
            0,
            TabView {
                path,
                zoom: self.zoom,
                viewport: (window.get_viewport_x(), window.get_viewport_y()),
                // Images are shared, so this copies no pixels; the old pages stay
                // on screen until the next document arrives. Only the pages on
                // screen are kept, so kept views stay small.
                cache: {
                    let mut cache = self.cache.clone();
                    cache.retain(&self.shown_pages.clone().unwrap_or(0..0));
                    cache
                },
            },
        );
        self.tab_views.truncate(KEPT_VIEWS);
    }

    pub fn opened(
        &mut self,
        window: &AppWindow,
        title: String,
        page_sizes: Vec<(f32, f32)>,
        outline: Vec<OutlineItem>,
    ) {
        self.remember_position();
        self.path = self.pending.take();
        let restored = self
            .path
            .as_deref()
            .and_then(|path| self.store.recent_for(path))
            .cloned();

        let kept = self.path.as_deref().and_then(|path| {
            let i = self.tab_views.iter().position(|v| v.path == path)?;
            Some(self.tab_views.remove(i))
        });

        self.title = title;
        self.page_sizes = page_sizes;
        self.outline = outline;
        self.labels.clear();
        window.set_page_label("".into());
        window.set_section_title("".into());
        self.zoom = kept
            .as_ref()
            .map(|v| v.zoom)
            .or(restored.as_ref().map(|r| r.zoom))
            .unwrap_or(self.default_zoom);
        self.current_page = 0;
        self.layout = Layout::default();
        self.thumb_layout = Layout::default();
        self.reset_document_state(window);
        self.annotations = self
            .path
            .as_deref()
            .map(Annotations::load)
            .unwrap_or_default();

        window.set_document_title(self.title.clone().into());
        window.set_page_count(ui_int(self.page_sizes.len()));
        window.set_current_page(0);
        window.set_status("".into());
        window.set_viewport_x(0.0);
        window.set_viewport_y(0.0);
        window.set_thumbs_viewport_y(0.0);
        if let Some(view) = kept {
            // Same pixels as when the user left: no blank pages, no jump.
            self.cache = view.cache;
            window.set_viewport_x(view.viewport.0);
            window.set_viewport_y(view.viewport.1);
            self.relayout(window);
        } else {
            self.relayout(window);
            if let Some(page) = restored.map(|r| r.page) {
                self.go_to_page(window, page.min(self.page_sizes.len().saturating_sub(1)));
            }
        }
        self.remember_position();
        self.update_recent();
        self.update_side_items();
        self.update_progress(window);
        let sections = self.sections().count();
        if self.annotations.outline_sections != sections
            && (self.has_notes() || !self.annotations.sections.is_empty())
        {
            self.annotations.outline_sections = sections;
            self.save_annotations(window);
        }
    }

    pub(super) fn reset_document_state(&mut self, window: &AppWindow) {
        self.cache.clear();
        self.last_request = None;
        self.thumb_cache.clear();
        self.last_thumb_request = None;
        self.selection = None;
        self.drag = None;
        self.popup = Popup::None;
        self.note_target = None;
        self.annotations = Annotations::default();
        self.history.clear();
        // A filter set for one document would hide colors in the next.
        self.hidden_colors.clear();
        self.close_search(window);
    }

    /// The worker couldn't open a document; returns the path it was asked for.
    pub fn open_failed(&mut self, window: &AppWindow, message: String) -> Option<PathBuf> {
        window.set_status(message.into());
        self.pending.take()
    }

    /// Saves the reading position of the open document.
    /// Records and saves the reading position now.
    pub fn remember_position(&mut self) {
        self.save_timer.stop();
        if self.record_position() {
            self.save();
        }
    }

    /// Records the reading position and saves it within `SAVE_DELAY`, so
    /// scrolling through pages writes the file at most once per delay.
    pub(super) fn remember_position_soon(&mut self) {
        if !self.record_position() || self.save_timer.running() {
            return;
        }
        self.save_timer
            .start(slint::TimerMode::SingleShot, SAVE_DELAY, || {
                crate::with_app(|app| app.viewer().save());
            });
    }

    pub(super) fn record_position(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        self.store.remember(Recent {
            path,
            page: self.current_page,
            zoom: self.zoom,
        });
        true
    }

    pub fn set_default_zoom(&mut self, zoom: DefaultZoom) {
        self.default_zoom = match zoom {
            DefaultZoom::FitWidth => Zoom::FitWidth,
            DefaultZoom::FitPage => Zoom::FitPage,
        };
    }

    /// Thumbnails are laid out for the sidebar width; re-render on change.
    pub fn set_sidebar_width(&mut self, window: &AppWindow, width: f32) {
        if width == self.sidebar_width {
            return;
        }
        self.sidebar_width = width;
        self.thumb_layout = Layout::default();
        self.thumb_cache.clear();
        self.last_thumb_request = None;
        self.relayout(window);
    }

    /// Recolors the pages: as drawn, inverted, or tinted like paper.
    pub fn set_page_tint(&mut self, window: &AppWindow, tint: PageTint) {
        // Config is reapplied on every settings change; most don't touch this.
        if tint == self.tint {
            return;
        }
        self.tint = tint;
        self.worker.send(Request::SetTint(tint));
        self.cache.clear();
        // Kept pages were rendered with the old colors.
        self.tab_views.clear();
        self.thumb_cache.clear();
        self.last_request = None;
        self.last_thumb_request = None;
        self.refresh(window);
    }
}
