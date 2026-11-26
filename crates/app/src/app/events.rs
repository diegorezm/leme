//! Events from the PDF worker, and calls through to the viewer.

use super::*;

impl App {
    pub fn event(&mut self, window: &AppWindow, event: Event<PageBuffer>) {
        match event {
            // A document opened after a newer one was requested is stale.
            Event::Opened {
                generation,
                ref path,
                ..
            } if generation != self.viewer.doc_gen() || !self.viewer.is_pending(path) => {}
            Event::Opened {
                title,
                page_sizes,
                outline,
                ..
            } => {
                self.viewer.opened(window, title, page_sizes, outline);
                if let Some((old, new)) = self.viewer.take_migrated_path() {
                    self.study.migrate_path(&old, &new);
                    let _ = self.study.save();
                    self.show_study_time(window);
                }
                self.tabs_changed(window);
                self.show_pending_article();
                if let Some((path, page, query)) = self.open_at.take() {
                    if self.viewer.is_showing(&path) {
                        self.show_hit(window, page, &query);
                    }
                }
            }
            Event::PasswordNeeded {
                generation,
                path,
                retry,
            } => {
                if generation == self.viewer.doc_gen() && self.viewer.is_pending(&path) {
                    self.ask_name(window, Prompt::Password(path), "");
                    if retry {
                        window.set_prompt_label(tr("Wrong password").into());
                    }
                }
            }
            Event::OpenFailed {
                generation,
                path,
                message,
            } => {
                if generation == self.viewer.doc_gen()
                    && self.viewer.open_failed(window, message).as_deref() == Some(path.as_path())
                {
                    if let Some(index) = self.tabs.iter().position(|t| *t == path) {
                        self.close_tab(window, index);
                    }
                }
            }
            Event::Labels { generation, labels } => {
                self.viewer.labels_found(window, generation, labels)
            }
            Event::Link { generation, target } => {
                if generation == self.viewer.doc_gen() {
                    if let Some(uri) = self.viewer.link(window, target) {
                        // Laws link to other laws: open those in Leme.
                        if crate::html::is_planalto(&uri) {
                            self.open_page(window, &uri);
                        } else {
                            // Only http(s) and mailto links get here (see pdf::safe_uri).
                            open_external(&uri);
                            self.flash(window, &tr("Opened link in browser"));
                        }
                    }
                }
            }
            Event::Rendered {
                generation,
                page,
                width_px,
                bitmap,
            } => self
                .viewer
                .rendered(window, generation, page, width_px, bitmap),
            Event::Thumbnail {
                generation,
                page,
                bitmap,
            } => self.viewer.thumbnail(generation, page, bitmap),
            Event::Cover {
                path,
                bitmap,
                pages,
            } => self.cover_rendered(window, path, bitmap, pages),
            Event::SearchHits { id, page, matches } => {
                self.viewer.search_hits(window, id, page, matches)
            }
            Event::SearchDone { id } => self.viewer.search_done(window, id),
            Event::Selection { generation, parts } => {
                self.viewer.selected(window, generation, parts)
            }
            Event::FileHit {
                id,
                path,
                page,
                snippet,
                count,
            } => self.file_hit(window, id, path, page, snippet, count),
            Event::FilesDone { .. } => {}
            Event::Article {
                generation,
                number,
                page,
            } => {
                if generation == self.viewer.doc_gen() {
                    self.article_found(window, number, page);
                }
            }
            Event::Typeset { path, result } => match result {
                // Highlights first find their text again, then it opens.
                Ok(()) => {
                    self.file_law(window, &path);
                    let wanted = self.page_for_library.take();
                    if let Some((url, library)) = wanted {
                        if web::source_of(&path).is_some_and(|s| s.url == url) {
                            self.libraries.add(library, &path);
                            self.libraries_changed(window);
                            self.viewer.request_covers(vec![path]);
                            return;
                        }
                        self.page_for_library = Some((url, library));
                    }
                    if !self.viewer.relocate_annotations(&path) {
                        self.reopen(window, path);
                    }
                }
                Err(err) => {
                    self.page_for_library = None;
                    self.flash(window, &trf("Couldn't save the page: {}", &[&err]));
                }
            },
            Event::Relocated { path, found } => {
                let (mut annotations, problem) = Annotations::load_checked(&path);
                if let Some(err) = problem {
                    self.flash(
                        window,
                        &trf(
                            "Couldn't read notes ({}); changes won't overwrite the file",
                            &[&err],
                        ),
                    );
                    self.reopen(window, path);
                    return;
                }
                let lost = annotations.relocate(&found);
                if let Err(err) = annotations.save(&path) {
                    self.flash(window, &trf("Couldn't save notes: {}", &[&err]));
                }
                self.reopen(window, path);
                if lost > 0 {
                    self.flash(
                        window,
                        &trf(
                            "{} highlights not found; they are listed last in notes",
                            &[&lost],
                        ),
                    );
                }
            }
            Event::CropDetected { generation, crop } => {
                self.viewer.crop_detected(window, generation, crop);
            }
            Event::Error(message) => {
                self.flash(window, &message);
            }
        }
    }

    // ----- Viewer passthrough ---------------------------------------------

    pub fn viewer(&mut self) -> &mut Viewer {
        &mut self.viewer
    }

    pub fn open_recent(&mut self, window: &AppWindow, index: usize) {
        if let Some(path) = self.viewer.store.recent.get(index).map(|r| r.path.clone()) {
            self.open_path(window, path);
        }
    }

    pub fn go_to_typed_page(&mut self, window: &AppWindow, text: &str) {
        self.set_overlay(window, "");
        // "art. 37" jumps to where that article of a law starts.
        match crate::nav::parse_article(text) {
            Some(number) => {
                self.viewer.find_article(number);
                window.set_status(trf("Looking for art. {}…", &[&number]).into());
            }
            None => self.viewer.go_to_typed_page(window, text),
        }
    }

    pub(super) fn article_found(&mut self, window: &AppWindow, number: u32, page: Option<usize>) {
        match page {
            Some(page) => {
                window.set_status("".into());
                self.viewer.jump_to_page(window, page);
            }
            None => self.flash(window, &trf("Art. {} not found", &[&number])),
        }
    }
}
