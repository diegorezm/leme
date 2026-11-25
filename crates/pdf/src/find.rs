//! Finding text: in the open document, in other documents, and where
//! annotations' texts moved to.

use crate::*;

/// Lowercase without accents, one char for one char, so positions in the
/// folded text match the original: "Licitação" and "licitacao" match.
pub(crate) fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        c if c.is_whitespace() => ' ',
        c => c,
    }
}

/// Matches of `query` (folded) in `text`, and the text around the first.
pub(crate) fn find_in_text(text: &str, query: &[char]) -> Option<(usize, String)> {
    if query.is_empty() {
        return None;
    }
    let folded: Vec<char> = text.chars().map(fold).collect();
    if folded.len() < query.len() {
        return None;
    }
    let mut first = None;
    let mut count = 0;
    for (i, window) in folded.windows(query.len()).enumerate() {
        if window == query {
            first.get_or_insert(i);
            count += 1;
        }
    }
    let first = first?;
    let from = first.saturating_sub(SNIPPET_CONTEXT);
    let to = (first + query.len() + SNIPPET_CONTEXT).min(folded.len());
    let snippet: String = text.chars().skip(from).take(to - from).collect();
    let mut snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if from > 0 {
        snippet.insert(0, '…');
    }
    if to < folded.len() {
        snippet.push('…');
    }
    Some((count, snippet))
}

/// Searches one page of a file search, or opens its next document.
/// Returns true when the search is finished.
pub(crate) fn search_files_step<'a, B>(
    pdfium: &'a Pdfium,
    job: &mut FilesJob<'a>,
    texts: &mut FileTexts,
    on_event: &impl Fn(Event<B>),
) -> bool {
    if job.hits >= MAX_FILE_HITS {
        return true;
    }
    let Some((path, page, doc)) = &mut job.current else {
        let Some(path) = job.paths.pop_front() else {
            return true;
        };
        if texts.pages(&path).is_none() {
            // Encrypted or broken documents are skipped.
            let Ok((doc, sizes)) = open(pdfium, &path, None) else {
                return false;
            };
            texts.insert(&path, sizes.len());
            job.current = Some((path, 0, Some(doc)));
        } else {
            job.current = Some((path, 0, None));
        }
        return false;
    };
    let Some(pages) = texts.pages_unchecked(path) else {
        job.current = None;
        return false;
    };
    let Some(slot) = pages.get_mut(*page) else {
        job.current = None;
        return false;
    };
    if slot.is_none() {
        if doc.is_none() {
            *doc = open(pdfium, path, None).ok().map(|(d, _)| d);
        }
        *slot = doc.as_ref().and_then(|d| page_text(d, *page));
    }
    if let Some((matches, snippet)) = slot.as_deref().and_then(|t| find_in_text(t, &job.query)) {
        job.hits += 1;
        on_event(Event::FileHit {
            id: job.id,
            path: path.clone(),
            page: *page,
            snippet,
            count: matches,
        });
    }
    *page += 1;
    false
}

/// Case-insensitive search of one page. Each match is one or more rects,
/// since a match can wrap across lines.
/// `cached` is the page's squashed text: when known, pages without the
/// query are skipped without loading them; when unknown, it is filled in.
pub(crate) fn search_page(
    doc: &PdfDocument,
    page: usize,
    query: &str,
    cached: Option<&mut Option<String>>,
) -> Result<Vec<Vec<NormRect>>, PdfiumError> {
    if let Some(Some(text)) = cached.as_deref() {
        if !text.contains(&squash(query)) {
            return Ok(Vec::new());
        }
    }
    let pdf_page = load_page(doc, page)?;
    let size = (pdf_page.width().value, pdf_page.height().value);
    let text = pdf_page.text()?;
    if let Some(slot @ None) = cached {
        *slot = Some(squash(&text.all()));
    }
    let search = text.search(query, &PdfSearchOptions::new())?;
    let matches = search
        .iter(PdfSearchDirection::SearchForward)
        .map(|segments| {
            segments
                .iter()
                .map(|segment| normalize(segment.bounds(), size))
                .collect()
        })
        .collect();
    Ok(matches)
}

/// Finds each text in the document, nearest its old page first, and
/// returns where: id, page, and the match's rects.
pub(crate) fn relocate(
    doc: &PdfDocument,
    page_count: usize,
    items: &[(u64, String, usize)],
) -> Vec<(u64, usize, Vec<NormRect>)> {
    let mut texts: Vec<Option<String>> = vec![None; page_count];
    let mut found = Vec::new();
    for (id, text, old_page) in items {
        let query = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            continue;
        }
        let mut pages: Vec<usize> = (0..page_count).collect();
        pages.sort_by_key(|p| p.abs_diff(*old_page));
        let hit = pages.into_iter().find_map(|page| {
            let matches = search_page(doc, page, &query, texts.get_mut(page)).ok()?;
            matches.into_iter().next().map(|rects| (page, rects))
        });
        if let Some((page, rects)) = hit {
            found.push((*id, page, rects));
        }
    }
    found
}
