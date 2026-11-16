//! Small shared helpers of the viewer.

use super::*;

impl Viewer {
    pub(super) fn save(&self) {
        // Losing a reading position is not worth interrupting the reader for.
        let _ = self.store.save();
    }

    pub fn update_recent(&self) {
        let items: Vec<RecentItem> = self
            .store
            .recent
            .iter()
            .take(RECENT_SHOWN)
            .map(|r| RecentItem {
                title: file_name(&r.path).into(),
                folder: r
                    .path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
                    .into(),
            })
            .collect();
        self.recent.set_vec(items);
    }

    /// Physical pixel width pages should be rendered at to look sharp.
    pub(super) fn target_width_px(&self, window: &AppWindow) -> u32 {
        let page_width = self.layout.rect(0).map_or(0.0, |r| r.width);
        (page_width * window.window().scale_factor())
            .round()
            .max(1.0) as u32
    }

    pub(super) fn page_views(&self, window: &AppWindow, pages: Range<usize>) -> Vec<PageView> {
        pages
            .filter_map(|page| self.page_view(window, page))
            .collect()
    }

    pub(super) fn page_view(&self, window: &AppWindow, page: usize) -> Option<PageView> {
        let scale = window.window().scale_factor();
        // Snap to physical pixels so a 1:1 render isn't resampled into blur.
        let snap = |v: f32| (v * scale).round() / scale;
        let rect = self.layout.rect(page)?;
        // Until the page renders, its thumbnail, stretched, beats a blank.
        let image = self
            .cache
            .get(page)
            .or_else(|| self.thumb_cache.get(&page))
            .cloned();
        Some(PageView {
            x: snap(rect.x),
            y: snap(rect.y),
            width: snap(rect.width),
            height: snap(rect.height),
            loaded: image.is_some(),
            image: image.unwrap_or_default(),
            highlights: self.highlights(page),
            marks: self.marks(page),
        })
    }

    pub(super) fn highlights(&self, page: usize) -> ModelRc<Highlight> {
        let Some(search) = &self.search else {
            return ModelRc::default();
        };
        let items: Vec<Highlight> = search
            .on_page(page)
            .map(|(r, current)| Highlight {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                current,
            })
            .collect();
        model_or_default(items)
    }

    pub(super) fn marks(&self, page: usize) -> ModelRc<Mark> {
        let mut items: Vec<Mark> = Vec::new();
        for a in self.annotations.on_page(page) {
            let color = self.shade(&a.color);
            let kind = match a.kind {
                Kind::Highlight => 0,
                Kind::Underline => 1,
                Kind::Strikethrough => 2,
            };
            let last = a.rects.len().saturating_sub(1);
            items.extend(a.rects.iter().enumerate().map(|(i, r)| Mark {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                kind,
                color,
                note: i == last && !a.note.is_empty(),
            }));
        }
        let selected = self.selection.iter().flat_map(|s| &s.parts);
        if let Some(part) = selected.into_iter().find(|p| p.page == page) {
            items.extend(part.rects.iter().map(|r| Mark {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                kind: 3,
                color: Color::default(),
                note: false,
            }));
        }
        model_or_default(items)
    }

    pub(super) fn request_renders(
        &mut self,
        window: &AppWindow,
        visible: Range<usize>,
        preload: Range<usize>,
    ) {
        if self.renders_held {
            return;
        }
        let width_px = self.target_width_px(window);
        let key = (preload.clone(), width_px);
        if self.last_request.as_ref() == Some(&key) {
            return;
        }
        // Visible pages first, then the ones just outside the viewport.
        let pages: Vec<usize> = visible
            .clone()
            .chain(preload.filter(|p| !visible.contains(p)))
            .filter(|&p| self.cache.width(p) != Some(width_px))
            .collect();
        // Visible pages with nothing to show yet get a quick preview first.
        let previews = visible
            .filter(|&p| !self.cache.contains(p) && !self.thumb_cache.contains_key(&p))
            .collect();
        self.last_request = Some(key);
        self.worker.send(Request::Render {
            previews,
            pages,
            width_px,
        });
    }
}
