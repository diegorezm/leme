//! Text selection and annotations.

use super::*;

impl Viewer {
    /// Mouse input on the reading area, in reader coordinates.
    pub fn pointer(&mut self, window: &AppWindow, kind: &str, x: f32, y: f32) {
        match kind {
            "down" => {
                self.drag = self.page_point(window, x, y).map(|(page, from)| Drag {
                    page,
                    from,
                    start: (x, y),
                    dragging: false,
                });
                if self.selection.take().is_some() || self.popup != Popup::None {
                    self.popup = Popup::None;
                    self.refresh(window);
                }
            }
            "move" => {
                let Some(drag) = &mut self.drag else {
                    return;
                };
                let moved = (x - drag.start.0).abs().max((y - drag.start.1).abs());
                if !drag.dragging && moved < DRAG_THRESHOLD {
                    return;
                }
                drag.dragging = true;
                let from = (drag.page, drag.from);
                let to = self.page_point_clamped(window, x, y);
                self.worker.send(Request::Select { from, to });
            }
            "up" => {
                let Some(drag) = self.drag.take() else {
                    return;
                };
                if drag.dragging {
                    let to = self.page_point_clamped(window, x, y);
                    self.worker.send(Request::Select {
                        from: (drag.page, drag.from),
                        to,
                    });
                    if self.selection.is_some() {
                        self.popup = Popup::Selection;
                    }
                } else if let Some(id) =
                    self.annotations
                        .at_point(drag.page, drag.from.0, drag.from.1)
                {
                    self.popup = Popup::Annotation(id);
                    window.set_annotation_has_note(
                        self.annotations.get(id).is_some_and(|a| !a.note.is_empty()),
                    );
                } else {
                    self.worker.send(Request::LinkAt {
                        page: drag.page,
                        point: drag.from,
                    });
                }
                self.refresh(window);
            }
            "hover" => self.hover(window, x, y),
            "double" => {
                if let Some((page, point)) = self.page_point(window, x, y) {
                    self.popup = Popup::None;
                    self.worker.send(Request::SelectLine { page, point });
                }
            }
            _ => {}
        }
    }

    /// Shows the note of the annotation under the mouse, if it has one.
    pub(super) fn hover(&mut self, window: &AppWindow, x: f32, y: f32) {
        let hit = self.page_point(window, x, y).and_then(|(page, (px, py))| {
            let id = self.annotations.at_point(page, px, py)?;
            let a = self.annotations.get(id)?;
            (!a.note.trim().is_empty()).then_some((id, a))
        });
        let id = hit.map(|(id, _)| id);
        if id == self.hovered {
            return;
        }
        self.hovered = id;
        let Some((_, a)) = hit else {
            window.set_note_tip("".into());
            return;
        };
        // Anchor below the annotation's last line.
        if let (Some(rect), Some(r)) = (self.layout.rect(a.page), a.rects.last()) {
            window.set_note_tip_x(
                rect.x + (r.x + r.width / 2.0) * rect.width + window.get_viewport_x(),
            );
            window
                .set_note_tip_y(rect.y + (r.y + r.height) * rect.height + window.get_viewport_y());
        }
        window.set_note_tip(a.note.trim().into());
    }

    pub fn selected(&mut self, window: &AppWindow, parts: Vec<SelectionPart>) {
        self.selection = (!parts.is_empty()).then_some(Selection { parts });
        // The final selection of a finished drag shows the toolbar.
        if self.drag.is_none() && self.selection.is_some() {
            self.popup = Popup::Selection;
        }
        self.refresh(window);
    }

    /// Clears the selection or popup. Returns whether there was one.
    pub fn clear_selection(&mut self, window: &AppWindow) -> bool {
        let had = self.selection.take().is_some() || self.popup != Popup::None;
        self.popup = Popup::None;
        if had {
            self.refresh(window);
        }
        had
    }

    /// Runs a selection- or annotation-toolbar button.
    pub fn popup_action(&mut self, window: &AppWindow, action: &str, index: usize) -> PopupOutcome {
        let color = annotations::COLORS
            .get(index)
            .map_or(annotations::COLORS[0].0, |(name, _)| name);
        let outcome = match (self.popup, action) {
            (Popup::Selection, "highlight") => {
                self.annotate(Kind::Highlight, color);
                PopupOutcome::Nothing
            }
            (Popup::Selection, "underline") => {
                self.annotate(Kind::Underline, "blue");
                PopupOutcome::Nothing
            }
            (Popup::Selection, "strike") => {
                self.annotate(Kind::Strikethrough, "red");
                PopupOutcome::Nothing
            }
            (Popup::Selection, "note") => match self.annotate(Kind::Highlight, "yellow") {
                Some(id) => self.edit_note(id),
                None => PopupOutcome::Nothing,
            },
            (Popup::Selection, "copy") => self.copy(),
            (Popup::Annotation(id), _) => self.annotation_action(id, action, color),
            _ => PopupOutcome::Nothing,
        };
        if !matches!(outcome, PopupOutcome::EditNote { .. }) {
            self.popup = Popup::None;
        }
        self.save_annotations(window);
        self.refresh(window);
        outcome
    }

    pub(super) fn annotation_action(&mut self, id: u64, action: &str, color: &str) -> PopupOutcome {
        match action {
            "delete" => {
                self.history.record(&self.annotations.items);
                self.annotations.remove(id);
            }
            "note" => return self.edit_note(id),
            "copy" => {
                let text = self.annotations.get(id).map(|a| a.text.clone());
                return text.map_or(PopupOutcome::Nothing, |t| copy_text(&t));
            }
            _ => {
                self.history.record(&self.annotations.items);
                if let Some(a) = self.annotations.get_mut(id) {
                    match action {
                        "color" => a.color = color.to_owned(),
                        "underline" => a.kind = Kind::Underline,
                        "strike" => a.kind = Kind::Strikethrough,
                        _ => {}
                    }
                }
            }
        }
        PopupOutcome::Nothing
    }

    /// A color key: highlights the selection, or recolors the clicked
    /// annotation, with color `index`.
    pub fn color_key(&mut self, window: &AppWindow, index: usize) -> PopupOutcome {
        match self.popup {
            Popup::Annotation(_) => self.popup_action(window, "color", index),
            _ if self.selection.is_some() => {
                self.popup = Popup::Selection;
                self.popup_action(window, "highlight", index)
            }
            _ => PopupOutcome::Nothing,
        }
    }

    /// Keyboard shortcuts for the selection (Copy, Highlight, AddNote).
    pub fn selection_action(&mut self, window: &AppWindow, action: Action) -> PopupOutcome {
        if self.selection.is_none() {
            if let (Popup::Annotation(_), Action::Copy) = (self.popup, action) {
                return self.popup_action(window, "copy", 0);
            }
            return PopupOutcome::Nothing;
        }
        self.popup = Popup::Selection;
        match action {
            Action::Copy => self.popup_action(window, "copy", 0),
            Action::Highlight => self.popup_action(window, "highlight", 0),
            Action::AddNote => self.popup_action(window, "note", 0),
            _ => PopupOutcome::Nothing,
        }
    }

    pub(super) fn annotate(&mut self, kind: Kind, color: &str) -> Option<u64> {
        let selection = self.selection.take()?;
        self.history.record(&self.annotations.items);
        // One annotation per page; a single undo step removes them all.
        let now = annotations::now();
        selection
            .parts
            .into_iter()
            .map(|part| {
                let id = self
                    .annotations
                    .add(part.page, kind, color, part.text, part.rects, now);
                if let Some(a) = self.annotations.get_mut(id) {
                    a.line = part.line;
                }
                id
            })
            .reduce(|first, _| first)
    }

    pub(super) fn edit_note(&mut self, id: u64) -> PopupOutcome {
        let Some(a) = self.annotations.get(id) else {
            return PopupOutcome::Nothing;
        };
        self.note_target = Some(id);
        self.popup = Popup::None;
        PopupOutcome::EditNote {
            quote: unhyphenated(&a.text),
            note: a.note.clone(),
            color: annotations::color_index(&a.color).unwrap_or(0),
            page: self.page_name(a.page),
        }
    }

    /// Recolors the annotation whose note is being edited. Returns false
    /// if no note is open.
    pub fn note_color(&mut self, window: &AppWindow, index: usize) -> bool {
        let Some((color, _)) = annotations::COLORS.get(index) else {
            return false;
        };
        let Some(id) = self.note_target else {
            return false;
        };
        if self.annotations.get(id).is_some_and(|a| a.color != *color) {
            self.history.record(&self.annotations.items);
            if let Some(a) = self.annotations.get_mut(id) {
                a.color = (*color).to_owned();
            }
            self.save_annotations(window);
            self.refresh(window);
        }
        true
    }

    pub(super) fn copy(&mut self) -> PopupOutcome {
        let text = self.selection.as_ref().map(Selection::text);
        self.selection = None;
        text.map_or(PopupOutcome::Nothing, |t| copy_text(&t))
    }

    pub fn note_saved(&mut self, window: &AppWindow, note: &str) {
        let note = note.trim();
        let target = self.note_target.take();
        let changed = target
            .and_then(|id| self.annotations.get(id))
            .is_some_and(|a| a.note != note);
        if changed {
            self.history.record(&self.annotations.items);
            if let Some(a) = target.and_then(|id| self.annotations.get_mut(id)) {
                a.note = note.to_owned();
            }
        }
        self.save_annotations(window);
        self.refresh(window);
    }

    /// Undoes (or redoes) the last annotation change. Returns false if
    /// there was nothing to undo.
    pub fn undo(&mut self, window: &AppWindow, redo: bool) -> bool {
        let restored = if redo {
            self.history.redo(&self.annotations.items)
        } else {
            self.history.undo(&self.annotations.items)
        };
        let Some(items) = restored else {
            return false;
        };
        self.annotations.items = items;
        self.selection = None;
        self.popup = Popup::None;
        self.save_annotations(window);
        self.refresh(window);
        true
    }

    pub fn note_cancelled(&mut self) {
        self.note_target = None;
    }

    pub(super) fn save_annotations(&mut self, window: &AppWindow) {
        match self.side_panel {
            SidePanel::Notes => self.update_side_items(),
            // Thumbnails show which colors each page has.
            SidePanel::Thumbs => self.update_thumbs(window, true),
            SidePanel::Outline => {}
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        self.annotations.document = path.display().to_string();
        if let Err(err) = self.annotations.save(&path) {
            window.set_status(trf("Couldn't save notes: {}", &[&err]).into());
        }
    }

    /// The notes in the shown colors as `format`, and how many there are.
    pub fn export(&self, format: export::Format, by_section: bool) -> (String, usize) {
        let file = self
            .path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let colors = self.shown_colors();
        let sections: Vec<(usize, String)> = self
            .sections()
            .map(|(page, title)| (page, title.to_owned()))
            .collect();
        let opts = export::Options {
            title: &self.title,
            file: &file,
            exported: annotations::now(),
            colors: &colors,
            names: &self.color_names,
            sections: &sections,
            by_section,
        };
        let count = export::selected(&self.annotations, &opts).len();
        (export::export(&self.annotations, format, &opts), count)
    }

    /// Converts a reader point to a page and a point in page fractions.
    pub(super) fn page_point(
        &self,
        window: &AppWindow,
        x: f32,
        y: f32,
    ) -> Option<(usize, (f32, f32))> {
        let cx = x - window.get_viewport_x();
        let cy = y - window.get_viewport_y();
        let page = self.layout.visible(cy, 0.01).start;
        let rect = self.layout.rect(page)?;
        let inside =
            cx >= rect.x && cx <= rect.x + rect.width && cy >= rect.y && cy <= rect.y + rect.height;
        inside.then(|| {
            (
                page,
                ((cx - rect.x) / rect.width, (cy - rect.y) / rect.height),
            )
        })
    }

    /// The page under a reader point (or nearest it) and the point in
    /// fractions of that page, clamped to the page.
    pub(super) fn page_point_clamped(
        &self,
        window: &AppWindow,
        x: f32,
        y: f32,
    ) -> (usize, (f32, f32)) {
        let cx = x - window.get_viewport_x();
        let cy = y - window.get_viewport_y();
        let page = self.layout.visible(cy, 0.01).start;
        let Some(rect) = self.layout.rect(page) else {
            return (page, (0.0, 0.0));
        };
        (
            page,
            (
                ((cx - rect.x) / rect.width).clamp(0.0, 1.0),
                ((cy - rect.y) / rect.height).clamp(0.0, 1.0),
            ),
        )
    }

    /// Positions the toolbar above the selection or annotation.
    pub(super) fn update_popup(&self, window: &AppWindow) {
        let anchor = match self.popup {
            Popup::None => None,
            Popup::Selection => self
                .selection
                .as_ref()
                .and_then(|s| s.parts.first())
                .and_then(|p| Some((p.page, *p.rects.first()?))),
            Popup::Annotation(id) => self
                .annotations
                .get(id)
                .and_then(|a| Some((a.page, *a.rects.first()?))),
        };
        let Some((page, r)) = anchor else {
            window.set_popup("".into());
            return;
        };
        let Some(rect) = self.layout.rect(page) else {
            return;
        };
        let x = rect.x + (r.x + r.width / 2.0) * rect.width + window.get_viewport_x();
        let y = rect.y + r.y * rect.height + window.get_viewport_y();
        window.set_popup_x(x);
        window.set_popup_y(y);
        window.set_popup(
            match self.popup {
                Popup::Selection => "selection",
                _ => "annotation",
            }
            .into(),
        );
    }
}
