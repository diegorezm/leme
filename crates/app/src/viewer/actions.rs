//! Scrolling, zooming, and page navigation.

use super::*;

impl Viewer {
    /// Handles document actions. Returns false for actions it doesn't own.
    pub fn handle(&mut self, window: &AppWindow, action: Action) -> bool {
        if self.page_sizes.is_empty() {
            return false;
        }
        match action {
            Action::ZoomIn => {
                let factor = self.zoom_factor(window);
                self.set_zoom(window, Zoom::zoom_in(factor));
            }
            Action::ZoomOut => {
                let factor = self.zoom_factor(window);
                self.set_zoom(window, Zoom::zoom_out(factor));
            }
            Action::ZoomReset | Action::FitWidth => self.set_zoom(window, Zoom::FitWidth),
            Action::FitPage => self.set_zoom(window, Zoom::FitPage),
            Action::ScrollDown => self.scroll_by(window, 0.0, SCROLL_STEP),
            Action::ScrollUp => self.scroll_by(window, 0.0, -SCROLL_STEP),
            Action::ScrollLeft => self.scroll_by(window, -SCROLL_STEP, 0.0),
            Action::ScrollRight => self.scroll_by(window, SCROLL_STEP, 0.0),
            Action::HalfPageDown => self.scroll_by(window, 0.0, window.get_reader_height() / 2.0),
            Action::HalfPageUp => self.scroll_by(window, 0.0, -window.get_reader_height() / 2.0),
            Action::NextMatch => self.search_step(window, true),
            Action::PreviousMatch => self.search_step(window, false),
            Action::PreviousPage | Action::NextPage | Action::FirstPage | Action::LastPage => {
                if let Some(page) =
                    nav::target_page(action, self.current_page, self.layout.page_count())
                {
                    self.go_to_page(window, page);
                }
            }
            _ => return false,
        }
        true
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }

    pub fn current_page(&self) -> usize {
        self.current_page
    }

    pub fn go_to_typed_page(&mut self, window: &AppWindow, text: &str) {
        let text = text.trim();
        // A label like "iv" or "A-3" wins over a page number.
        let labelled = self
            .labels
            .iter()
            .position(|label| !label.is_empty() && label.eq_ignore_ascii_case(text));
        if let Some(page) = labelled.or_else(|| nav::parse_page(text, self.page_sizes.len())) {
            self.go_to_page(window, page);
        }
    }

    pub fn go_to_page(&mut self, window: &AppWindow, page: usize) {
        self.scroll_to(window, self.layout.scroll_to(page));
        self.refresh(window);
    }

    pub(super) fn set_zoom(&mut self, window: &AppWindow, zoom: Zoom) {
        if zoom == self.zoom {
            return;
        }
        // Relayout moves the current page, so remember it first.
        let page = self.current_page;
        self.zoom = zoom;
        // In a burst of zoom steps (Ctrl+wheel, held key) only the last
        // zoom is rendered; the ones between stretch the previous render.
        if self.zoom_timer.running() {
            self.renders_held = true;
        }
        let weak = window.as_weak();
        self.zoom_timer
            .start(slint::TimerMode::SingleShot, ZOOM_SETTLE, move || {
                if let Some(window) = weak.upgrade() {
                    crate::with_app(|app| app.viewer().resize_settled(&window));
                }
            });
        self.relayout(window);
        if zoom == Zoom::FitPage {
            self.go_to_page(window, page);
        }
        self.remember_position_soon();
    }

    /// Scale relative to fit width for the current zoom mode.
    pub(super) fn zoom_factor(&self, window: &AppWindow) -> f32 {
        let page = self
            .page_sizes
            .get(self.current_page)
            .or(self.page_sizes.first())
            .copied()
            .unwrap_or((1.0, 1.0));
        let viewport = (window.get_reader_width(), window.get_reader_height());
        self.zoom.factor(page, viewport)
    }

    pub(super) fn update_zoom_display(&self, window: &AppWindow) {
        let width_px = self.layout.rect(0).map_or(0.0, |r| r.width);
        let width_pt = self.page_sizes.first().map_or(0.0, |s| s.0);
        window.set_zoom_percent(layout::percent(width_px, width_pt));
        window.set_zoom_mode(
            match self.zoom {
                Zoom::FitWidth => "fit-width",
                Zoom::FitPage => "fit-page",
                Zoom::Custom(_) => "custom",
            }
            .into(),
        );
    }

    pub(super) fn scroll_to(&self, window: &AppWindow, top: f32) {
        let max = (self.layout.content_height() - window.get_reader_height()).max(0.0);
        window.set_viewport_y(-top.clamp(0.0, max));
    }

    pub(super) fn scroll_by(&mut self, window: &AppWindow, dx: f32, dy: f32) {
        if dy != 0.0 {
            self.scroll_to(window, -window.get_viewport_y() + dy);
        }
        if dx != 0.0 {
            window.set_viewport_x(window.get_viewport_x() - dx);
            self.clamp_x(window);
        }
        self.refresh(window);
    }

    pub(super) fn clamp_x(&self, window: &AppWindow) {
        let max = (self.layout.content_width() - window.get_reader_width()).max(0.0);
        window.set_viewport_x(-(-window.get_viewport_x()).clamp(0.0, max));
    }

    /// Scrolls the sidebar so the current page's thumbnail is visible.
    pub(super) fn reveal_thumb(&self, window: &AppWindow, page: usize) {
        let Some(rect) = self.thumb_layout.rect(page) else {
            return;
        };
        let height = window.get_reader_height();
        let top = -window.get_thumbs_viewport_y();
        if rect.y >= top && rect.y + rect.height <= top + height {
            return;
        }
        let max = (self.thumb_layout.content_height() - height).max(0.0);
        let target = rect.y - (height - rect.height) / 2.0;
        window.set_thumbs_viewport_y(-target.clamp(0.0, max));
    }
}
