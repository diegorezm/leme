//! Web pages saved as documents.

use super::*;

impl App {
    /// The address the front tab was saved from, if it's a web page.
    pub(super) fn active(&self) -> Option<PathBuf> {
        self.tabs.get(self.active?).cloned()
    }

    /// Opens a web page as a document: the saved copy if there is one,
    /// else downloads it.
    pub(super) fn open_page(&mut self, window: &AppWindow, uri: &str) {
        let (page, fragment) = uri.split_once('#').unwrap_or((uri, ""));
        let Some(url) = web::address(page) else {
            return;
        };
        if let Some(article) = library::anchor_article(fragment) {
            self.pending_article = Some((None, Some(url.clone()), article));
        }
        let saved = config::data_dir().and_then(|d| web::saved(&d.join("pages"), &url));
        match saved {
            Some(pdf) => self.open_path(window, pdf),
            None => {
                self.flash(window, &tr("Downloading page…"));
                let source = web::Source {
                    url,
                    revoked: self.config.show_revoked,
                };
                crate::load_page(window, source, None);
            }
        }
    }

    /// Opens `path` again after it was rewritten.
    pub(super) fn reopen(&mut self, window: &AppWindow, path: PathBuf) {
        self.viewer.forget(&path);
        self.open_path(window, path);
    }

    /// Where the front tab was saved from, if it's a web page.
    pub(super) fn active_source(&self) -> Option<web::Source> {
        web::source_of(&self.active()?)
    }

    /// A page downloaded (or read again from disk): keep it and typeset it
    /// into the pages folder.
    pub fn page_loaded(
        &mut self,
        window: &AppWindow,
        source: web::Source,
        result: Result<(Article, Vec<u8>), String>,
    ) {
        let (article, html) = match result {
            Ok(page) => page,
            Err(err) => {
                self.flash(window, &trf("Couldn't download the page: {}", &[&err]));
                return;
            }
        };
        let Some(pages) = config::data_dir().map(|d| d.join("pages")) else {
            self.flash(window, &tr("No data folder to save the page in"));
            return;
        };
        let (dir, pdf) = web::target(&pages, &source.url, &article.title);
        if let Err(err) = web::save(&dir, &source, &html) {
            self.flash(window, &trf("Couldn't save the page: {}", &[&err]));
            return;
        }
        self.viewer.typeset(article, source.url, pdf);
    }
}
