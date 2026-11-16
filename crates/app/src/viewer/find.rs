//! Search in the open document.

use super::*;

impl Viewer {
    pub fn search_edited(&mut self, window: &AppWindow, query: &str) {
        self.next_search_id += 1;
        let id = self.next_search_id;
        let query = query.trim().to_owned();
        self.search = (!query.is_empty()).then(|| SearchState::new(id, self.current_page));
        window.set_search_status("".into());
        self.worker.send(Request::Search {
            id,
            query,
            from: self.current_page,
        });
        self.refresh(window);
    }

    pub fn search_step(&mut self, window: &AppWindow, forward: bool) {
        let Some(search) = &mut self.search else {
            return;
        };
        if forward {
            search.next();
        } else {
            search.previous();
        }
        self.reveal_match(window);
    }

    pub fn search_hits(
        &mut self,
        window: &AppWindow,
        id: u64,
        page: usize,
        matches: Vec<Vec<NormRect>>,
    ) {
        let Some(search) = self.search.as_mut().filter(|s| s.id == id) else {
            return;
        };
        if search.add(page, matches) {
            self.reveal_match(window);
        } else {
            // Hits stream in page by page, on every page for a short query:
            // only this page's highlights change.
            self.update_search_status(window);
            self.refresh_page(window, page);
        }
    }

    pub fn search_done(&mut self, window: &AppWindow, id: u64) {
        let Some(search) = self.search.as_mut().filter(|s| s.id == id) else {
            return;
        };
        if search.finish() {
            self.reveal_match(window);
        } else {
            self.update_search_status(window);
        }
    }

    pub fn close_search(&mut self, window: &AppWindow) {
        if self.search.take().is_some() {
            self.next_search_id += 1;
            self.worker.send(Request::Search {
                id: self.next_search_id,
                query: String::new(),
                from: 0,
            });
        }
        window.set_search_text("".into());
        window.set_search_status("".into());
        self.refresh(window);
    }

    /// Scrolls the current match into view if it isn't already.
    pub(super) fn reveal_match(&mut self, window: &AppWindow) {
        self.update_search_status(window);
        let target = self
            .search
            .as_ref()
            .and_then(|s| s.current())
            .and_then(|m| {
                let page = self.layout.rect(m.page)?;
                let first = m.rects.first()?;
                Some(page.y + first.y * page.height)
            });
        if let Some(y) = target {
            let top = -window.get_viewport_y();
            let height = window.get_reader_height();
            let margin = 48.0;
            if y < top + margin || y > top + height - margin {
                self.scroll_to(window, y - height / 3.0);
            }
        }
        self.refresh(window);
    }

    pub(super) fn update_search_status(&self, window: &AppWindow) {
        let status = self.search.as_ref().map(|s| s.status()).unwrap_or_default();
        window.set_search_status(status.into());
    }
}
