//! Command palette entries and fuzzy matching. Pure and UI-free.

use std::path::PathBuf;

use crate::keymap::Action;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Command(Action),
    Tab(usize),
    Recent(PathBuf),
    GoToPage(usize),
    Theme(String),
    Session(usize),
    /// The native file dialog.
    ChooseFile,
    /// A web page to save as a document.
    Page(String),
}

/// What the palette lists an entry under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// "Go to page 42": on its own at the top, without a heading.
    Page,
    Themes,
    Commands,
    Tabs,
    Sessions,
}

impl Group {
    /// The heading shown above the group, in English.
    pub fn title(self) -> &'static str {
        match self {
            Group::Page => "",
            Group::Themes => "Themes",
            Group::Commands => "Commands",
            Group::Tabs => "Tabs and recent",
            Group::Sessions => "Sessions",
        }
    }
}

impl Target {
    pub fn group(&self) -> Group {
        match self {
            Target::GoToPage(_) | Target::ChooseFile | Target::Page(_) => Group::Page,
            Target::Theme(_) => Group::Themes,
            Target::Command(_) => Group::Commands,
            Target::Tab(_) | Target::Recent(_) => Group::Tabs,
            Target::Session(_) => Group::Sessions,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub target: Target,
    pub title: String,
    /// Muted text after the title, e.g. a folder or "Tab".
    pub detail: String,
    /// Shortcut shown on the right.
    pub keys: String,
}

/// A PDF path typed or pasted into the palette, without the quotes a file
/// manager's "copy as path" adds.
pub fn typed_path(query: &str) -> Option<PathBuf> {
    let path = query.trim().trim_matches('"').trim();
    let pdf = path
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
    let absolute =
        path.starts_with('/') || path.starts_with("\\\\") || path.as_bytes().get(1) == Some(&b':');
    (pdf && absolute && path.len() > 4).then(|| PathBuf::from(path))
}

/// Fuzzy subsequence score: higher is better, `None` if `query` doesn't
/// match. Rewards consecutive letters, word starts, and early matches.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut qi = 0;
    let mut previous: Option<usize> = None;
    for (ti, &c) in text.iter().enumerate() {
        if qi == query.len() {
            break;
        }
        if c != query[qi] {
            continue;
        }
        let word_start = ti == 0 || !text[ti - 1].is_alphanumeric();
        score += 1;
        if word_start {
            score += 8;
        }
        if previous == Some(ti.wrapping_sub(1)) {
            score += 5;
        }
        if ti < 3 {
            score += 3;
        }
        previous = Some(ti);
        qi += 1;
    }
    (qi == query.len()).then(|| score - text.len() as i32 / 8)
}

/// Entries matching `query`, best first. Ties keep the original order,
/// so callers list what matters most first.
pub fn filter(entries: &[Entry], query: &str, limit: usize) -> Vec<Entry> {
    let mut scored: Vec<(i32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let title = score(query, &e.title);
            let detail = score(query, &e.detail).map(|s| s / 2);
            title.max(detail).map(|s| (s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, i)| entries[i].clone())
        .collect()
}

/// Brings each group's entries together, keeping the groups in the order
/// their best entry appeared and the entries' order within each, so the
/// best match stays first and every heading shows once. A group shows at
/// most `per_group` entries, so one kind (a dozen themes) can't crowd out
/// the rest; the list has at most `limit`.
pub fn grouped(entries: Vec<Entry>, per_group: usize, limit: usize) -> Vec<Entry> {
    let mut groups: Vec<(Group, Vec<Entry>)> = Vec::new();
    for entry in entries {
        let group = entry.target.group();
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, members)) => {
                if members.len() < per_group {
                    members.push(entry);
                }
            }
            None => groups.push((group, vec![entry])),
        }
    }
    let mut shown: Vec<Entry> = groups.into_iter().flat_map(|(_, m)| m).collect();
    shown.truncate(limit);
    shown
}

/// "42" or ":42" means "go to page 42".
pub fn page_query(query: &str) -> Option<usize> {
    let q = query.trim().trim_start_matches(':');
    q.parse::<usize>().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str) -> Entry {
        Entry {
            target: Target::Command(Action::Open),
            title: title.to_owned(),
            detail: String::new(),
            keys: String::new(),
        }
    }

    #[test]
    fn subsequence_matching() {
        assert!(score("zin", "Zoom in").is_some());
        assert!(score("zi", "Fit width").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("ZOOM", "zoom out").is_some());
    }

    #[test]
    fn ranks_word_starts_and_prefixes() {
        let entries = [
            entry("Toggle focus mode"),
            entry("Fit page"),
            entry("Go to page…"),
        ];
        let ranked = filter(&entries, "fp", 10);
        assert_eq!(ranked[0].title, "Fit page");
        let ranked = filter(&entries, "page", 10);
        assert_eq!(ranked.len(), 2);
        assert_eq!(filter(&entries, "", 2).len(), 2);
    }

    #[test]
    fn groups_stay_together_best_first() {
        let with = |target, title: &str| Entry {
            target,
            ..entry(title)
        };
        let ranked = vec![
            with(Target::Theme("Noite".into()), "a"),
            with(Target::Command(Action::Open), "b"),
            with(Target::Theme("Velino".into()), "c"),
            with(Target::Tab(0), "d"),
            with(Target::Command(Action::Search), "e"),
        ];
        let titles =
            |entries: Vec<Entry>| -> Vec<String> { entries.into_iter().map(|e| e.title).collect() };
        assert_eq!(
            titles(grouped(ranked.clone(), 5, 10)),
            ["a", "c", "b", "e", "d"]
        );
        // One per group, then the overall limit.
        assert_eq!(titles(grouped(ranked.clone(), 1, 10)), ["a", "b", "d"]);
        assert_eq!(titles(grouped(ranked, 5, 3)), ["a", "c", "b"]);
        assert!(grouped(Vec::new(), 5, 10).is_empty());
    }

    #[test]
    fn page_queries() {
        assert_eq!(page_query("42"), Some(42));
        assert_eq!(page_query(":7"), Some(7));
        assert_eq!(page_query("0"), None);
        assert_eq!(page_query("open"), None);
    }

    #[test]
    fn typed_paths() {
        assert_eq!(
            typed_path(r#" "C:\Docs\Lei 8112.PDF" "#),
            Some(PathBuf::from(r"C:\Docs\Lei 8112.PDF"))
        );
        assert_eq!(
            typed_path("/home/a/b.pdf"),
            Some(PathBuf::from("/home/a/b.pdf"))
        );
        assert_eq!(typed_path("b.pdf"), None);
        assert_eq!(typed_path("C:\\notes.txt"), None);
        assert_eq!(typed_path("mazza"), None);
    }
}
