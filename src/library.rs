//! Libraries: named collections of documents (PDFs on disk and saved web
//! pages) shared by every session, each with an icon or a picture of its
//! own. Stored in `libraries.toml` in the data folder. Pure and UI-free.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Icons a library can have, by name; the UI draws them.
pub const ICONS: [&str; 10] = [
    "book", "scale", "bookmark", "star", "folder", "code", "flask", "globe", "pencil", "heart",
];

/// Where saved laws go unless the user moves them.
pub const LAWS: &str = "Leis";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    /// One of `ICONS`.
    #[serde(default = "default_icon")]
    pub icon: String,
    /// A picture of the user's, shown instead of the icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub documents: Vec<PathBuf>,
}

fn default_icon() -> String {
    ICONS[0].to_owned()
}

impl Library {
    pub fn new(name: &str, icon: &str) -> Self {
        Library {
            name: name.to_owned(),
            icon: icon.to_owned(),
            image: None,
            documents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Libraries {
    #[serde(default, rename = "library")]
    pub items: Vec<Library>,
}

impl Libraries {
    /// Reads `path`; a missing or broken file gives no libraries, and the
    /// error for broken ones.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match fs::read_to_string(path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(libraries) => (libraries, None),
                Err(err) => (Self::default(), Some(format!("libraries.toml: {err}"))),
            },
            Err(_) => (Self::default(), None),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|l| l.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Adds a library named `name`, unless one exists; returns its index.
    pub fn create(&mut self, name: &str, icon: &str) -> Option<usize> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if let Some(i) = self.find(name) {
            return Some(i);
        }
        self.items.push(Library::new(name, icon));
        Some(self.items.len() - 1)
    }

    pub fn rename(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        let taken = self.find(name).is_some_and(|i| i != index);
        match self.items.get_mut(index) {
            Some(library) if !name.is_empty() && !taken => {
                library.name = name.to_owned();
                true
            }
            _ => false,
        }
    }

    /// Adds `doc` to library `index`; `false` if it was already there.
    pub fn add(&mut self, index: usize, doc: &Path) -> bool {
        match self.items.get_mut(index) {
            Some(library) if !library.documents.iter().any(|d| d == doc) => {
                library.documents.push(doc.to_owned());
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, index: usize, doc: &Path) -> bool {
        match self.items.get_mut(index) {
            Some(library) => {
                let before = library.documents.len();
                library.documents.retain(|d| d != doc);
                library.documents.len() != before
            }
            None => false,
        }
    }

    /// Libraries holding `doc`.
    pub fn holding(&self, doc: &Path) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].documents.iter().any(|d| d == doc))
            .collect()
    }

    /// Every document with the library it's in, for searching.
    pub fn documents(&self) -> impl Iterator<Item = (&Library, &PathBuf)> {
        self.items
            .iter()
            .flat_map(|l| l.documents.iter().map(move |d| (l, d)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_add_remove() {
        let mut libs = Libraries::default();
        let laws = libs.create(" Leis ", "scale").unwrap();
        assert_eq!(libs.create("leis", "book"), Some(laws));
        assert_eq!(libs.create("  ", "book"), None);
        let other = libs.create("Artigos", "globe").unwrap();
        assert!(!libs.rename(other, "LEIS"));
        assert!(libs.rename(other, "Papers"));

        let cf = Path::new("/data/pages/1/CF.pdf");
        assert!(libs.add(laws, cf));
        assert!(!libs.add(laws, cf));
        assert!(libs.add(other, cf));
        assert_eq!(libs.holding(cf), [laws, other]);
        assert_eq!(libs.documents().count(), 2);
        assert!(libs.remove(other, cf));
        assert!(!libs.remove(other, cf));
        assert_eq!(libs.holding(cf), [laws]);
    }

    #[test]
    fn file_round_trip_and_lenient() {
        let dir = std::env::temp_dir().join(format!("leme-libs-{}", std::process::id()));
        let path = dir.join("libraries.toml");
        let mut libs = Libraries::default();
        let i = libs.create("Leis", "scale").unwrap();
        libs.items[i].image = Some(PathBuf::from("/pics/lei.png"));
        libs.add(i, Path::new("/a.pdf"));
        libs.save(&path).unwrap();
        assert_eq!(Libraries::load(&path), (libs, None));

        fs::write(&path, "[[library]]\nname = \"Só nome\"\n").unwrap();
        let (loaded, problem) = Libraries::load(&path);
        assert!(problem.is_none());
        assert_eq!(loaded.items[0].icon, "book");

        fs::write(&path, "not toml [").unwrap();
        let (loaded, problem) = Libraries::load(&path);
        assert!(loaded.items.is_empty() && problem.is_some());
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Libraries::load(&path).0, Libraries::default());
    }
}
