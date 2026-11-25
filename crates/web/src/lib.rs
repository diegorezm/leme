//! Web pages saved as documents. A page is fetched only when the user
//! pastes its address, through the system's curl like the update check,
//! then kept as a PDF in the data folder with the address it came from,
//! so it reads offline and can be fetched again to update it.
//!
//! Layout: `<data>/pages/<hash of address>/<title>.pdf` and `source.txt`
//! (the address on the first line). The PDF keeps its name on updates,
//! so highlights and reading position stay attached to it.

pub mod html;
pub mod update;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

/// Largest page fetched; law codes run to a few MB.
const MAX_BYTES: &str = "40000000";

/// The address to fetch, if `text` looks like one. Plain `http` is
/// upgraded: only HTTPS is fetched.
pub fn address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.contains(char::is_whitespace) {
        return None;
    }
    let rest = if let Some(rest) = text.strip_prefix("https://") {
        rest
    } else if let Some(rest) = text.strip_prefix("http://") {
        rest
    } else if text.starts_with("www.") {
        text
    } else {
        return None;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (host.contains('.') && host.len() > 3).then(|| format!("https://{rest}"))
}

/// Downloads `url`. Blocks; call it off the UI thread.
pub fn fetch(url: &str) -> io::Result<Vec<u8>> {
    let output = crate::update::curl()
        .args(["-L", "--compressed", "--max-filesize", MAX_BYTES])
        .args(["--retry", "2"])
        // A generic browser agent: some sites (planalto.gov.br) drop
        // connections from curl's own or a bare "Mozilla/5.0".
        .args(["-A", USER_AGENT])
        .arg(url)
        .output()?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(io::Error::other(if reason.is_empty() {
            "download failed".to_owned()
        } else {
            reason
        }));
    }
    Ok(output.stdout)
}

/// The PDF of a page saved from `url`, if there is one.
pub fn saved(pages_dir: &Path, url: &str) -> Option<PathBuf> {
    let (_, pdf) = target(pages_dir, url, "");
    pdf.is_file().then_some(pdf)
}

/// Where the page from `url` is kept: its folder, and the PDF in it. An
/// existing PDF keeps its name; a new one is named after `title`.
pub fn target(pages_dir: &Path, url: &str, title: &str) -> (PathBuf, PathBuf) {
    let dir = pages_dir.join(format!("{:016x}", domain::config::fnv1a(url.as_bytes())));
    let existing = fs::read_dir(&dir).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
    });
    let pdf = existing.unwrap_or_else(|| dir.join(format!("{}.pdf", file_name(title, url))));
    (dir, pdf)
}

/// A saved page: where it came from, and how its revoked
/// (struck-through) text is shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub url: String,
    /// Shows revoked text struck through instead of hiding it.
    pub revoked: bool,
}

/// Saves the downloaded page and where it came from next to its PDF.
pub fn save(dir: &Path, source: &Source, html: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let html_path = dir.join(HTML);
    let tmp = html_path.with_extension("tmp");
    fs::write(&tmp, html)?;
    fs::rename(tmp, html_path)?;
    let mode = if source.revoked {
        "revoked = shown"
    } else {
        "revoked = hidden"
    };
    domain::config::write_atomic(
        &dir.join("source.txt"),
        &format!("{}\n{mode}\n", source.url),
    )
}

const HTML: &str = "page.html";

/// Where a saved page came from, if `pdf` is one.
pub fn source_of(pdf: &Path) -> Option<Source> {
    let text = fs::read_to_string(pdf.parent()?.join("source.txt")).ok()?;
    let mut lines = text.lines();
    let url = address(lines.next()?)?;
    let revoked = lines.any(|l| l.trim() == "revoked = shown");
    Some(Source { url, revoked })
}

/// The downloaded page kept next to `pdf`, to typeset it again offline.
pub fn saved_html(pdf: &Path) -> Option<Vec<u8>> {
    fs::read(pdf.parent()?.join(HTML)).ok()
}

/// A file name from the page title, else from the address.
fn file_name(title: &str, url: &str) -> String {
    let base = if title.trim().is_empty() {
        url.trim_start_matches("https://")
    } else {
        title
    };
    let name: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = name.chars().take(80).collect();
    let name = name.trim_end_matches(['.', ' ']).to_owned();
    if name.is_empty() {
        "page".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let cf = "https://www.planalto.gov.br/ccivil_03/constituicao/constituicao.htm";
        assert_eq!(address(&format!("  {cf} ")).as_deref(), Some(cf));
        assert_eq!(
            address("http://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(
            address("www.example.com").as_deref(),
            Some("https://www.example.com")
        );
        assert_eq!(address("example.com"), None);
        assert_eq!(address("C:\\a.pdf"), None);
        assert_eq!(address("https://x"), None);
        assert_eq!(address("https://a.com/b c"), None);
    }

    #[test]
    fn names_and_folders() {
        assert_eq!(
            file_name("Lei nº 8.112: regime / jurídico", ""),
            "Lei nº 8.112 regime jurídico"
        );
        assert_eq!(file_name("  ", "https://a.com/x"), "a.com x");
        assert_eq!(file_name("...", ""), "page");
        let dir = std::env::temp_dir().join("leme-web-test-none");
        let (folder, pdf) = target(&dir, "https://a.com", "Título");
        assert_eq!(pdf, folder.join("Título.pdf"));
        assert_eq!(
            domain::config::fnv1a(b"https://a.com"),
            domain::config::fnv1a(b"https://a.com")
        );
        assert_ne!(
            domain::config::fnv1a(b"https://a.com"),
            domain::config::fnv1a(b"https://b.com")
        );
    }

    #[test]
    fn source_round_trip() {
        let dir = std::env::temp_dir().join(format!("leme-web-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = Source {
            url: "https://a.com/x".to_owned(),
            revoked: true,
        };
        save(&dir, &source, b"<p>x</p>").unwrap();
        let pdf = dir.join("x.pdf");
        assert_eq!(source_of(&pdf), Some(source));
        assert_eq!(saved_html(&pdf).as_deref(), Some(&b"<p>x</p>"[..]));
        fs::remove_dir_all(&dir).unwrap();
    }
}
