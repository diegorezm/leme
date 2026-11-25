//! Layout, the render cache, and what the worker draws next.

use super::*;

impl Viewer {
    pub fn rendered(
        &mut self,
        window: &AppWindow,
        generation: u64,
        page: usize,
        width_px: u32,
        bitmap: PageBuffer,
    ) {
        if generation != self.render_gen {
            return;
        }
        // A render for an outdated width still beats a blank page.
        let stale = width_px != self.target_width_px(window);
        if stale && self.cache.contains(page) {
            return;
        }
        let bytes = bitmap.as_bytes().len();
        let visible = self.visible.clone();
        self.cache
            .insert(page, width_px, bytes, Image::from_rgb8(bitmap), &visible);
        self.refresh_page(window, page);
    }

    /// Rebuilds one page's item, if shown. Rebuilding them all would
    /// recreate every page's image, highlight, and mark items.
    pub(super) fn refresh_page(&mut self, window: &AppWindow, page: usize) {
        let row = self
            .shown_pages
            .as_ref()
            .filter(|shown| shown.contains(&page))
            .map(|shown| page - shown.start);
        if let (Some(row), Some(view)) = (row, self.page_view(window, page)) {
            self.pages.set_row_data(row, view);
        }
    }

    pub fn thumbnail(&mut self, generation: u64, page: usize, bitmap: PageBuffer) {
        if generation != self.render_gen {
            return;
        }
        self.thumb_cache.insert(page, Image::from_rgb8(bitmap));
        let row = self
            .shown_thumbs
            .as_ref()
            .filter(|shown| shown.contains(&page))
            .map(|shown| page - shown.start);
        if let (Some(row), Some(view)) = (row, self.thumb_view(page)) {
            self.thumbs.set_row_data(row, view);
        }
    }

    /// Recomputes page positions after a resize, zoom, or new document.
    pub fn resized(&mut self, window: &AppWindow) {
        self.renders_held = true;
        self.relayout(window);
    }

    pub fn resize_settled(&mut self, window: &AppWindow) {
        self.renders_held = false;
        self.refresh(window);
    }

    pub fn relayout(&mut self, window: &AppWindow) {
        if self.page_sizes.is_empty() {
            return;
        }
        let old_height = self.layout.content_height();
        let top = -window.get_viewport_y();

        let factor = self.zoom_factor(window);
        self.layout = Layout::new(&self.page_sizes, window.get_reader_width(), factor);
        self.update_zoom_display(window);
        window.set_content_width(self.layout.content_width());
        window.set_content_height(self.layout.content_height());

        // Keep the same relative reading position.
        if old_height > 0.0 {
            let ratio = top / old_height;
            self.scroll_to(window, ratio * self.layout.content_height());
        }
        self.clamp_x(window);

        if self.thumb_layout.page_count() != self.page_sizes.len() {
            self.thumb_layout = Layout::new(&self.page_sizes, self.sidebar_width, 1.0);
            window.set_thumbs_content_height(self.thumb_layout.content_height());
        }
        self.refresh(window);
    }

    /// Updates visible pages, the page counter, and render requests after
    /// anything changed.
    pub fn refresh(&mut self, window: &AppWindow) {
        self.update(window, true);
    }

    /// Like `refresh`, for scrolling: page items are only rebuilt when a
    /// different set of pages comes into range, so frames stay cheap.
    pub fn scrolled(&mut self, window: &AppWindow) {
        // The tooltip would drift off its annotation.
        if self.hovered.take().is_some() {
            window.set_note_tip("".into());
        }
        self.update(window, false);
    }

    pub(super) fn update(&mut self, window: &AppWindow, force: bool) {
        let count = self.layout.page_count();
        if count == 0 {
            self.pages.set_vec(Vec::new());
            self.thumbs.set_vec(Vec::new());
            self.shown_pages = None;
            self.shown_thumbs = None;
            window.set_popup("".into());
            return;
        }
        let top = -window.get_viewport_y();
        let height = window.get_reader_height();
        let visible = self.layout.visible(top, height);
        let preload = layout::with_margin(visible.clone(), PRELOAD, count);
        let keep = layout::with_margin(visible.clone(), PRELOAD + KEEP, count);

        let current = self.layout.current_page(top, height);
        if current != self.current_page || window.get_current_page() == 0 {
            self.current_page = current;
            window.set_current_page(ui_int(current + 1));
            self.update_page_label(window);
            self.update_section_title(window);
            if self.side_panel == SidePanel::Outline {
                self.update_side_items();
            }
            self.reveal_thumb(window, current);
            // Saved soon after every page change so the position survives
            // a crash, without writing the file on each page scrolled past.
            self.remember_position_soon();
        }
        self.cache.retain(&keep);
        self.visible = visible.clone();
        if force || self.shown_pages.as_ref() != Some(&preload) {
            self.pages.set_vec(self.page_views(window, preload.clone()));
            self.shown_pages = Some(preload.clone());
        }
        self.request_renders(window, visible, preload);
        self.update_thumbs(window, force);
        self.update_popup(window);
    }

    pub fn refresh_thumbs(&mut self, window: &AppWindow) {
        self.update_thumbs(window, true);
    }

    pub fn thumbs_scrolled(&mut self, window: &AppWindow) {
        self.update_thumbs(window, false);
    }

    pub(super) fn update_thumbs(&mut self, window: &AppWindow, force: bool) {
        let count = self.thumb_layout.page_count();
        if count == 0 || !window.get_sidebar_visible() {
            return;
        }
        let top = -window.get_thumbs_viewport_y();
        let visible = self.thumb_layout.visible(top, window.get_reader_height());
        let preload = layout::with_margin(visible.clone(), THUMB_PRELOAD, count);
        let keep = layout::with_margin(visible.clone(), THUMB_KEEP, count);

        self.thumb_cache.retain(|page, _| keep.contains(page));
        if !force && self.shown_thumbs.as_ref() == Some(&preload) {
            return;
        }
        self.shown_thumbs = Some(preload.clone());
        let views: Vec<ThumbView> = preload.clone().filter_map(|p| self.thumb_view(p)).collect();
        self.thumbs.set_vec(views);

        if self.last_thumb_request.as_ref() != Some(&preload) {
            let width = self.thumb_layout.rect(0).map_or(1.0, |r| r.width);
            let width_px = (width * window.window().scale_factor()).round().max(1.0) as u32;
            let pages = visible
                .clone()
                .chain(preload.clone().filter(|p| !visible.contains(p)))
                .filter(|p| !self.thumb_cache.contains_key(p))
                .collect();
            self.worker.send(Request::Thumbnails { pages, width_px });
            self.last_thumb_request = Some(preload);
        }
    }

    pub(super) fn thumb_view(&self, page: usize) -> Option<ThumbView> {
        let rect = self.thumb_layout.rect(page)?;
        let image = self.thumb_cache.get(&page).cloned();
        Some(ThumbView {
            page: ui_int(page),
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            loaded: image.is_some(),
            image: image.unwrap_or_default(),
            dots: self.page_dots(page),
        })
    }

    /// The distinct annotation colors on a page, in palette order.
    pub(super) fn page_dots(&self, page: usize) -> ModelRc<Color> {
        let colors: Vec<Color> = annotations::COLORS
            .iter()
            .filter(|(name, _)| self.annotations.on_page(page).any(|a| a.color == *name))
            .take(3)
            .map(|(name, _)| self.shade(name))
            .collect();
        model_or_default(colors)
    }

    pub fn sidebar_shown(&mut self, window: &AppWindow) {
        self.last_thumb_request = None;
        self.refresh_thumbs(window);
    }
}
