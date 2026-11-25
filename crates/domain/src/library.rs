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
        crate::config::write_atomic(path, &text)
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

// ----- Law shortcuts ("cf 37") ------------------------------------------------

const PLANALTO: &str = "https://www.planalto.gov.br/ccivil_03/";

/// Built-in shortcuts: what to type, the law's name, and its address on
/// planalto.gov.br. `[aliases]` in config.toml adds more or replaces these.
pub const ALIASES: [(&str, &str, &str); 8] = [
    (
        "cf",
        "Constituição Federal",
        "constituicao/constituicao.htm",
    ),
    ("cc", "Código Civil", "leis/2002/l10406compilada.htm"),
    ("cp", "Código Penal", "decreto-lei/del2848compilado.htm"),
    (
        "cpc",
        "Código de Processo Civil",
        "_ato2015-2018/2015/lei/l13105.htm",
    ),
    (
        "cpp",
        "Código de Processo Penal",
        "decreto-lei/del3689compilado.htm",
    ),
    (
        "clt",
        "Consolidação das Leis do Trabalho",
        "decreto-lei/del5452compilado.htm",
    ),
    (
        "ctn",
        "Código Tributário Nacional",
        "leis/l5172compilado.htm",
    ),
    (
        "cdc",
        "Código de Defesa do Consumidor",
        "leis/l8078compilado.htm",
    ),
];

/// The built-in shortcut `key`: name and address.
pub fn alias(key: &str) -> Option<(&'static str, String)> {
    ALIASES
        .iter()
        .find(|(k, _, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, name, path)| (*name, format!("{PLANALTO}{path}")))
}

/// "cf 37", "cf art. 5º", "8112 art 41", "8.112": a law's shortcut or
/// number, and maybe an article.
#[derive(Debug, Clone, PartialEq)]
pub struct LawQuery {
    /// Lowercase shortcut, or the law's number without dots.
    pub key: String,
    pub article: Option<u32>,
}

impl LawQuery {
    pub fn is_number(&self) -> bool {
        self.key.chars().all(|c| c.is_ascii_digit())
    }
}

pub fn law_query(query: &str) -> Option<LawQuery> {
    let mut words = query.split_whitespace();
    let key = words.next()?.to_lowercase();
    let key = if key.chars().all(|c| c.is_ascii_digit() || c == '.') {
        key.replace('.', "")
    } else {
        key
    };
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric()) {
        return None;
    }
    let rest: String = words.collect::<Vec<_>>().join(" ").to_lowercase();
    let rest = rest
        .trim_start_matches("art")
        .trim_start_matches('.')
        .trim();
    let article = if rest.is_empty() {
        None
    } else {
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let tail = rest[digits.len()..].trim_matches(|c| matches!(c, 'º' | '°' | 'o' | '-'));
        if !tail.is_empty() {
            return None;
        }
        Some(digits.replace('.', "").parse().ok()?)
    };
    Some(LawQuery { key, article })
}

/// A law number as laws write it: 8112 → "8.112".
pub fn dotted(number: &str) -> String {
    let digits: Vec<char> = number.chars().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(*c);
    }
    out
}

/// The article a Planalto anchor points to: "art5", "art37a" → 5, 37.
pub fn anchor_article(fragment: &str) -> Option<u32> {
    let rest = fragment.to_lowercase();
    let rest = rest.strip_prefix("art")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
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

    #[test]
    fn law_queries() {
        let q = |s: &str| law_query(s).map(|q| (q.key, q.article));
        assert_eq!(q("cf 37"), Some(("cf".into(), Some(37))));
        assert_eq!(q("CF art. 5º"), Some(("cf".into(), Some(5))));
        assert_eq!(q("8112 art 41"), Some(("8112".into(), Some(41))));
        assert_eq!(q("8.112"), Some(("8112".into(), None)));
        assert_eq!(q("cpc 1.015"), Some(("cpc".into(), Some(1015))));
        assert_eq!(q("cf trinta"), None);
        assert_eq!(q("C:\\a.pdf"), None);
        assert_eq!(q(""), None);
        assert!(law_query("8112").unwrap().is_number());
        assert!(!law_query("cf").unwrap().is_number());
    }
}
