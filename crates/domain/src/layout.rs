//! Geometry of the continuous page strip, in logical pixels. Pure and UI-free.

use std::ops::Range;

/// Vertical gap between pages and around the strip.
pub const GAP: f32 = 16.0;
/// Horizontal margin on each side of the pages at fit-width.
pub const SIDE_MARGIN: f32 = 24.0;

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 5.0;
pub const ZOOM_STEP: f32 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
struct RowBounds {
    y: f32,
    bottom: f32,
    start_page: usize,
    end_page: usize,
}

/// Page positions for a document laid out top to bottom, in single-page or
/// side-by-side (`two_up`) rows, every page scaled to the same width.
#[derive(Debug, Default)]
pub struct Layout {
    rects: Vec<PageRect>,
    rows: Vec<RowBounds>,
    content_width: f32,
    content_height: f32,
}

impl Layout {
    /// `page_sizes` are page dimensions in points; `zoom` 1.0 means fit width.
    pub fn new(page_sizes: &[(f32, f32)], viewport_width: f32, zoom: f32) -> Self {
        Self::new_with_mode(page_sizes, viewport_width, zoom, false)
    }

    /// Lays out pages either stacked one per row (`two_up = false`) or in
    /// side-by-side spreads of two (`two_up = true`).
    pub fn new_with_mode(
        page_sizes: &[(f32, f32)],
        viewport_width: f32,
        zoom: f32,
        two_up: bool,
    ) -> Self {
        let cols = if two_up { 2 } else { 1 };
        let fit_width = fit_page_width(viewport_width, two_up);
        let page_width = (fit_width * zoom).max(1.0);
        let spread_width = if two_up && page_sizes.len() > 1 {
            page_width * 2.0 + GAP
        } else {
            page_width
        };
        let content_width = viewport_width.max(spread_width + 2.0 * SIDE_MARGIN);
        let x_start = (content_width - spread_width) / 2.0;

        let mut rects = Vec::with_capacity(page_sizes.len());
        let mut rows = Vec::with_capacity(page_sizes.len().div_ceil(cols));
        let mut y = GAP;
        for (row_idx, chunk) in page_sizes.chunks(cols).enumerate() {
            let start_page = row_idx * cols;
            let mut row_height = 0.0_f32;
            for (col, &(w, h)) in chunk.iter().enumerate() {
                let aspect = if w > 0.0 && h > 0.0 { h / w } else { 1.0 };
                let height = page_width * aspect;
                row_height = row_height.max(height);
                rects.push(PageRect {
                    x: x_start + (col as f32) * (page_width + GAP),
                    y,
                    width: page_width,
                    height,
                });
            }
            let bottom = y + row_height;
            rows.push(RowBounds {
                y,
                bottom,
                start_page,
                end_page: start_page + chunk.len(),
            });
            y = bottom + GAP;
        }

        Self {
            rects,
            rows,
            content_width,
            content_height: y,
        }
    }

    pub fn rect(&self, page: usize) -> Option<PageRect> {
        self.rects.get(page).copied()
    }

    pub fn page_count(&self) -> usize {
        self.rects.len()
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// Pages intersecting `[top, top + height)`.
    pub fn visible(&self, top: f32, height: f32) -> Range<usize> {
        let bottom = top + height;
        let start_row = self.rows.partition_point(|r| r.bottom <= top);
        let end_row = self.rows.partition_point(|r| r.y < bottom);
        let start = self
            .rows
            .get(start_row)
            .map_or(self.rects.len(), |r| r.start_page);
        let end = if end_row <= start_row {
            start
        } else {
            self.rows[end_row - 1].end_page
        };
        start..end
    }

    /// The page shown at the vertical middle of the viewport.
    pub fn current_page(&self, top: f32, height: f32) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let middle = top + height / 2.0;
        let index = self
            .rows
            .partition_point(|r| r.bottom + GAP <= middle)
            .min(self.rows.len().saturating_sub(1));
        self.rows[index].start_page
    }

    /// Scroll offset that puts `page` at the top of the viewport.
    pub fn scroll_to(&self, page: usize) -> f32 {
        self.rect(page).map_or(0.0, |r| r.y - GAP)
    }
}

fn fit_page_width(viewport_width: f32, two_up: bool) -> f32 {
    let strip = (viewport_width - 2.0 * SIDE_MARGIN).max(1.0);
    if two_up {
        ((strip - GAP) / 2.0).max(1.0)
    } else {
        strip
    }
}

/// Expands `range` by `margin` pages on each side, clamped to `count`.
pub fn with_margin(range: Range<usize>, margin: usize, count: usize) -> Range<usize> {
    range.start.saturating_sub(margin)..(range.end + margin).min(count)
}

/// Zoom (relative to fit width) at which a page of `page_size` points fits
/// entirely in the viewport.
pub fn fit_page_zoom(page_size: (f32, f32), viewport: (f32, f32)) -> f32 {
    fit_page_zoom_in_mode(page_size, viewport, false)
}

/// Like [`fit_page_zoom`], accounting for single vs two-up page width.
pub fn fit_page_zoom_in_mode(page_size: (f32, f32), viewport: (f32, f32), two_up: bool) -> f32 {
    let (page_w, page_h) = page_size;
    let fit_width = fit_page_width(viewport.0, two_up);
    if page_w <= 0.0 || page_h <= 0.0 {
        return 1.0;
    }
    let height_at_fit_width = fit_width * page_h / page_w;
    let available = (viewport.1 - 2.0 * GAP).max(1.0);
    (available / height_at_fit_width).clamp(MIN_ZOOM, 1.0)
}

/// How pages are scaled. The fit modes are recomputed on every resize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    FitWidth,
    FitPage,
    /// A factor relative to fit width.
    Custom(f32),
}

impl Zoom {
    /// The scale factor relative to fit width, for the page being read.
    pub fn factor(self, page_size: (f32, f32), viewport: (f32, f32)) -> f32 {
        self.factor_in_mode(page_size, viewport, false)
    }

    /// Like [`Zoom::factor`], accounting for single vs two-up mode.
    pub fn factor_in_mode(self, page_size: (f32, f32), viewport: (f32, f32), two_up: bool) -> f32 {
        match self {
            Zoom::FitWidth => 1.0,
            Zoom::FitPage => fit_page_zoom_in_mode(page_size, viewport, two_up),
            Zoom::Custom(factor) => factor.clamp(MIN_ZOOM, MAX_ZOOM),
        }
    }

    pub fn zoom_in(factor: f32) -> Self {
        Zoom::Custom((factor * ZOOM_STEP).min(MAX_ZOOM))
    }

    pub fn zoom_out(factor: f32) -> Self {
        Zoom::Custom((factor / ZOOM_STEP).max(MIN_ZOOM))
    }

    pub fn to_token(self) -> String {
        match self {
            Zoom::FitWidth => "fit-width".to_owned(),
            Zoom::FitPage => "fit-page".to_owned(),
            Zoom::Custom(factor) => factor.to_string(),
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "fit-width" => Some(Zoom::FitWidth),
            "fit-page" => Some(Zoom::FitPage),
            _ => {
                let factor: f32 = token.parse().ok()?;
                (factor.is_finite() && factor > 0.0).then_some(Zoom::Custom(factor))
            }
        }
    }
}

/// Zoom as a percentage of the page's real size (1pt = 1/72in, 96 px/in).
pub fn percent(page_width_px: f32, page_width_pt: f32) -> i32 {
    if page_width_pt <= 0.0 {
        return 100;
    }
    (page_width_px / (page_width_pt * 96.0 / 72.0) * 100.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    // Three US Letter pages in a viewport whose fit width is 612.
    fn letter() -> Layout {
        Layout::new(&[(612.0, 792.0); 3], 612.0 + 2.0 * SIDE_MARGIN, 1.0)
    }

    #[test]
    fn stacks_pages_with_gaps() {
        let layout = letter();
        assert_eq!(layout.rect(0).map(|r| (r.y, r.height)), Some((GAP, 792.0)));
        assert_eq!(layout.rect(1).map(|r| r.y), Some(GAP * 2.0 + 792.0));
        assert_eq!(layout.content_height(), GAP * 4.0 + 792.0 * 3.0);
        assert_eq!(layout.rect(0).map(|r| r.x), Some(SIDE_MARGIN));
    }

    #[test]
    fn visible_range() {
        let layout = letter();
        assert_eq!(layout.visible(0.0, 500.0), 0..1);
        assert_eq!(layout.visible(700.0, 300.0), 0..2);
        assert_eq!(layout.visible(10_000.0, 500.0), 3..3);
    }

    #[test]
    fn current_page_follows_middle() {
        let layout = letter();
        assert_eq!(layout.current_page(0.0, 600.0), 0);
        assert_eq!(layout.current_page(layout.scroll_to(1), 600.0), 1);
        assert_eq!(layout.current_page(1e9, 600.0), 2);
        assert_eq!(Layout::default().current_page(0.0, 600.0), 0);
    }

    #[test]
    fn zoom_widens_content() {
        let layout = Layout::new(&[(100.0, 100.0)], 248.0, 2.0);
        assert_eq!(layout.rect(0).map(|r| r.width), Some(400.0));
        assert_eq!(layout.content_width(), 448.0);
        assert_eq!(Zoom::zoom_in(MAX_ZOOM), Zoom::Custom(MAX_ZOOM));
        assert_eq!(Zoom::zoom_out(MIN_ZOOM), Zoom::Custom(MIN_ZOOM));
    }

    #[test]
    fn fit_page() {
        // Fit width 612 makes the page 792 tall; 400 available => ~0.505.
        let zoom = fit_page_zoom(
            (612.0, 792.0),
            (612.0 + 2.0 * SIDE_MARGIN, 400.0 + 2.0 * GAP),
        );
        assert!((zoom - 400.0 / 792.0).abs() < 1e-4);
        // A wide page already fits at fit width.
        assert_eq!(fit_page_zoom((800.0, 100.0), (500.0, 800.0)), 1.0);
    }

    #[test]
    fn zoom_modes() {
        let viewport = (612.0 + 2.0 * SIDE_MARGIN, 400.0 + 2.0 * GAP);
        assert_eq!(Zoom::FitWidth.factor((612.0, 792.0), viewport), 1.0);
        assert!(Zoom::FitPage.factor((612.0, 792.0), viewport) < 1.0);
        assert_eq!(Zoom::Custom(99.0).factor((1.0, 1.0), viewport), MAX_ZOOM);
        for zoom in [Zoom::FitWidth, Zoom::FitPage, Zoom::Custom(1.5)] {
            assert_eq!(Zoom::from_token(&zoom.to_token()), Some(zoom));
        }
        assert_eq!(Zoom::from_token("-1"), None);
        assert_eq!(Zoom::from_token("abc"), None);
    }

    #[test]
    fn two_up_pairs_pages_in_rows() {
        let vp_w = 200.0 + 2.0 * SIDE_MARGIN + GAP;
        let layout = Layout::new_with_mode(&[(100.0, 200.0); 3], vp_w, 1.0, true);
        let r0 = layout.rect(0).expect("page 0");
        let r1 = layout.rect(1).expect("page 1");
        let r2 = layout.rect(2).expect("page 2");
        assert_eq!(r0.width, 100.0);
        assert_eq!(r0.y, r1.y);
        assert_eq!(r1.x, r0.x + 100.0 + GAP);
        assert_eq!(r2.y, r0.y + 200.0 + GAP);
        assert_eq!(layout.visible(0.0, 150.0), 0..2);
        assert_eq!(layout.visible(250.0, 150.0), 2..3);
        assert_eq!(layout.current_page(layout.scroll_to(2), 100.0), 2);
    }
}
