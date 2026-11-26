//! The side panel (thumbnails, outline, notes), page labels, and links.

use super::*;

impl Viewer {
    pub fn side_panel(&self) -> SidePanel {
        self.side_panel
    }

    pub fn set_side_panel(&mut self, window: &AppWindow, panel: SidePanel) {
        self.side_panel = panel;
        window.set_side_panel(panel.name().into());
        self.update_side_items();
        if panel == SidePanel::Thumbs {
            self.shown_thumbs = None;
            self.update_thumbs(window, true);
        }
    }

    pub(super) fn update_side_items(&mut self) {
        self.side_outline.clear();
        self.current_outline_row = None;
        let (items, pages): (Vec<SideItem>, Vec<usize>) = match self.side_panel {
            SidePanel::Thumbs => Default::default(),
            SidePanel::Outline => {
                let current = self.current_section();
                let levels: HashMap<(usize, &str), u8> = self
                    .annotations
                    .sections
                    .iter()
                    .map(|s| ((s.page, s.title.as_str()), s.level))
                    .collect();
                let side_outline = &mut self.side_outline;
                let current_row = &mut self.current_outline_row;
                self.outline
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| {
                        let page = item.page?;
                        let is_current = current == Some(i);
                        if is_current {
                            *current_row = Some(side_outline.len());
                        }
                        side_outline.push(i);
                        let status = levels
                            .get(&(page, item.title.as_str()))
                            .copied()
                            .unwrap_or(0);
                        let row = SideItem {
                            title: item.title.as_str().into(),
                            depth: ui_int(item.depth.min(6)),
                            current: is_current,
                            status: i32::from(status),
                            ..Default::default()
                        };
                        Some((row, page))
                    })
                    .unzip()
            }
            SidePanel::Notes => {
                let mut notes: Vec<_> = self
                    .annotations
                    .items
                    .iter()
                    .filter(|a| !self.hidden_colors.contains(&a.color))
                    .collect();
                notes.sort_by(|a, b| {
                    a.is_lost()
                        .cmp(&b.is_lost())
                        .then(a.page.cmp(&b.page))
                        .then_with(|| {
                            let y =
                                |n: &annotations::Annotation| n.rects.first().map_or(0.0, |r| r.y);
                            y(a).total_cmp(&y(b))
                        })
                });
                // Notes are listed under the outline section they are in.
                let mut section = None;
                notes
                    .into_iter()
                    .map(|a| {
                        // Lost notes go last, under one heading, as if a
                        // section past the last.
                        let here = if a.is_lost() {
                            Some(usize::MAX)
                        } else {
                            self.section_at(a.page)
                        };
                        let header = match here {
                            Some(usize::MAX) if section != here => tr("Not found after updating"),
                            Some(i) if section != here => self.outline[i].title.clone(),
                            _ => String::new(),
                        };
                        section = here;
                        let row = SideItem {
                            title: self.color_name(&a.color).into(),
                            color: self.shade(&a.color),
                            status: -1,
                            header: header.into(),
                            page: trf("p. {}", &[&self.page_name(a.page)]).into(),
                            quote: truncate(&unhyphenated(&a.text), 160).into(),
                            note: truncate(&a.note, 240).into(),
                            ..Default::default()
                        };
                        (row, a.page)
                    })
                    .unzip()
            }
        };
        self.side_items.set_vec(items);
        self.side_pages = pages;
        self.update_note_filters();
    }

    /// Moves the active outline highlight without rebuilding the whole list.
    pub(super) fn update_current_outline_row(&mut self) {
        let next = self
            .current_section()
            .and_then(|idx| self.side_outline.iter().position(|&o| o == idx));
        if next == self.current_outline_row {
            return;
        }
        if let Some(old) = self.current_outline_row {
            if let Some(mut row) = self.side_items.row_data(old) {
                row.current = false;
                self.side_items.set_row_data(old, row);
            }
        }
        if let Some(new) = next {
            if let Some(mut row) = self.side_items.row_data(new) {
                row.current = true;
                self.side_items.set_row_data(new, row);
            }
        }
        self.current_outline_row = next;
    }

    /// The colors used in this document, with counts, for the notes filter.
    pub(super) fn update_note_filters(&self) {
        let filters: Vec<NoteFilter> = self
            .used_colors()
            .map(|(i, color, count)| NoteFilter {
                name: self.color_names.get(i).map_or(color, String::as_str).into(),
                color: self.shade(color),
                count: ui_int(count),
                on: !self.hidden_colors.contains(color),
            })
            .collect();
        self.note_filters.set_vec(filters);
        if let Some(window) = self.window.upgrade() {
            let total = self.annotations.items.len();
            window.set_notes_all_label(trf("All · {}", &[&total]).into());
        }
    }

    /// What a highlight color means, as named in Settings.
    pub(super) fn color_name<'a>(&'a self, color: &'a str) -> &'a str {
        annotations::color_index(color)
            .and_then(|i| self.color_names.get(i))
            .map_or(color, String::as_str)
    }

    /// Sets the highlight colors' names and shades, in `COLORS` order.
    pub fn set_colors(&mut self, window: &AppWindow, names: Vec<String>, shades: Vec<Color>) {
        // Config is reapplied on every settings change; most don't touch colors.
        let unchanged = names == self.color_names && shades == self.color_shades;
        if unchanged && window.get_annotation_colors().row_count() > 0 {
            return;
        }
        self.color_names = names;
        self.color_shades = shades;
        window.set_annotation_colors(Rc::new(VecModel::from(self.color_shades.clone())).into());
        self.update_side_items();
        self.update_thumbs(window, true);
        self.refresh(window);
    }

    /// The shade a highlight color is drawn in.
    pub(super) fn shade(&self, color: &str) -> Color {
        annotations::color_index(color)
            .and_then(|i| self.color_shades.get(i).copied())
            .unwrap_or_else(|| to_color(annotations::color_hex(color)))
    }

    /// A color picked in the notes filter; `index` counts only the colors
    /// in use, like the chips. From "all", it shows that color alone;
    /// after that, colors toggle, and hiding the last one shows all again.
    pub fn note_filter_clicked(&mut self, index: usize) {
        let used: Vec<&str> = self.used_colors().map(|(_, color, _)| color).collect();
        let Some(&picked) = used.get(index) else {
            return;
        };
        self.hidden_colors = toggle_filter(&used, &self.hidden_colors, picked);
        self.update_side_items();
    }

    /// Shows every color in the notes list again.
    pub fn note_filter_all(&mut self) {
        self.hidden_colors.clear();
        self.update_side_items();
    }

    /// The colors this document uses, in palette order: index, name, and
    /// how many annotations have it.
    pub(super) fn used_colors(&self) -> impl Iterator<Item = (usize, &'static str, usize)> + '_ {
        annotations::COLORS
            .iter()
            .enumerate()
            .map(|(i, (color, _))| {
                let count = self
                    .annotations
                    .items
                    .iter()
                    .filter(|a| a.color == *color)
                    .count();
                (i, *color, count)
            })
            .filter(|(_, _, count)| *count > 0)
    }

    pub fn has_notes(&self) -> bool {
        !self.annotations.items.is_empty()
    }

    /// Colors the notes list currently shows.
    pub fn shown_colors(&self) -> Vec<String> {
        annotations::COLORS
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .filter(|name| !self.hidden_colors.contains(name))
            .collect()
    }

    /// Advances an outline section's progress from its checkbox.
    pub fn side_item_status_clicked(&mut self, window: &AppWindow, index: usize) {
        if let Some(&i) = self.side_outline.get(index) {
            self.cycle_section(window, i);
        }
    }

    /// Advances the progress of the section the reader is in.
    pub fn mark_current_section(&mut self, window: &AppWindow) -> bool {
        match self.current_section() {
            Some(i) => {
                self.cycle_section(window, i);
                true
            }
            None => false,
        }
    }

    /// The outline entry the reader is in: the last one starting at or
    /// before the current page.
    pub(super) fn current_section(&self) -> Option<usize> {
        self.section_at(self.current_page)
    }

    /// The outline entry `page` belongs to.
    pub(super) fn section_at(&self, page: usize) -> Option<usize> {
        self.outline
            .iter()
            .enumerate()
            .filter_map(|(i, item)| Some((i, item.page?)))
            .filter(|(_, start)| *start <= page)
            // Of several starting on the same page, the first: it is the
            // one at the top, where reading resumes.
            .rev()
            .max_by_key(|(_, start)| *start)
            .map(|(i, _)| i)
    }

    /// Shows the section the reader is in next to the page number.
    pub(super) fn update_section_title(&self, window: &AppWindow) {
        let title = self
            .current_section()
            .and_then(|i| self.outline.get(i))
            .map_or("", |item| item.title.as_str());
        window.set_section_title(title.into());
    }

    pub(super) fn cycle_section(&mut self, window: &AppWindow, outline_index: usize) {
        let Some((Some(page), title)) = self
            .outline
            .get(outline_index)
            .map(|item| (item.page, item.title.clone()))
        else {
            return;
        };
        self.annotations.cycle_level(page, &title);
        self.annotations.outline_sections = self.sections().count();
        self.save_annotations(window);
        self.update_side_items();
        self.update_progress(window);
    }

    /// The outline sections that count toward progress: those with a page.
    pub(super) fn sections(&self) -> impl Iterator<Item = (usize, &str)> {
        self.outline
            .iter()
            .filter_map(|item| Some((item.page?, item.title.as_str())))
    }

    /// How many sections are read and how many there are.
    pub fn progress(&self) -> (usize, usize) {
        self.annotations.progress(self.sections())
    }

    pub(super) fn update_progress(&self, window: &AppWindow) {
        let (done, total) = self.progress();
        let text = match total {
            0 => String::new(),
            1 => trf("{} of 1 section read", &[&done]),
            _ => trf("{} of {} sections read", &[&done, &total]),
        };
        let reviewed = self
            .sections()
            .filter(|(page, title)| self.annotations.level(*page, title) > 1)
            .count();
        window.set_outline_progress_text(text.into());
        window.set_outline_reviewed_text(match reviewed {
            0 => "".into(),
            n => trf("{} reviewed", &[&n]).into(),
        });
        window.set_outline_progress(if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        });
    }

    pub fn side_item_clicked(&mut self, window: &AppWindow, index: usize) {
        if let Some(&page) = self.side_pages.get(index) {
            self.jump_to_page(window, page);
        }
    }

    pub fn labels_found(&mut self, window: &AppWindow, generation: u64, labels: Vec<String>) {
        if generation == self.doc_gen && labels.len() == self.page_sizes.len() {
            self.labels = labels;
            self.update_page_label(window);
            self.update_side_items();
        }
    }

    /// The label of a page if it has one, else its number.
    pub(super) fn page_name(&self, page: usize) -> String {
        match self.labels.get(page) {
            Some(label) if !label.is_empty() => label.clone(),
            _ => (page + 1).to_string(),
        }
    }

    pub(super) fn update_page_label(&self, window: &AppWindow) {
        let name = self.page_name(self.current_page);
        let shown = if name == (self.current_page + 1).to_string() {
            String::new()
        } else {
            name
        };
        window.set_page_label(shown.into());
    }

    /// Follows a clicked link. Returns an external URI for the caller to open.
    pub fn link(&mut self, window: &AppWindow, target: Option<LinkTarget>) -> Option<String> {
        match target? {
            LinkTarget::Page(page) if page < self.page_sizes.len() => {
                self.jump_to_page(window, page);
                None
            }
            LinkTarget::Page(_) => None,
            LinkTarget::Uri(uri) => Some(uri),
        }
    }
}
