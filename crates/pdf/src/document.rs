//! Opening documents and reading what they declare: outline, links, page
//! labels.

use std::path::Path;

use crate::*;

/// Prefers the pdfium.dll next to the executable, then the system library.
pub fn bind() -> Result<Pdfium, PdfiumError> {
    let local = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .map(Pdfium::pdfium_platform_library_name_at_path)
        })
        .map(|path| Pdfium::bind_to_library(path));
    let bindings = match local {
        Some(Ok(bindings)) => bindings,
        _ => Pdfium::bind_to_system_library()?,
    };
    Ok(Pdfium::new(bindings))
}

pub(crate) fn open<'a>(
    pdfium: &'a Pdfium,
    path: &Path,
    password: Option<&str>,
) -> Result<(PdfDocument<'a>, Vec<(f32, f32)>), PdfiumError> {
    let doc = pdfium.load_pdf_from_file(path, password)?;
    // Reads sizes without loading each page, so large documents open fast.
    let sizes = doc
        .pages()
        .page_sizes()?
        .iter()
        .map(|rect| (rect.width().value, rect.height().value))
        .collect();
    Ok((doc, sizes))
}

/// Huge or cyclic outlines are cut off rather than walked forever.
pub(crate) const MAX_OUTLINE_ITEMS: usize = 5000;
pub(crate) const MAX_OUTLINE_DEPTH: usize = 16;

pub(crate) fn outline(doc: &PdfDocument) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    walk_outline(doc.bookmarks().root(), 0, &mut items);
    items
}

pub(crate) fn walk_outline(
    mut node: Option<PdfBookmark>,
    depth: usize,
    items: &mut Vec<OutlineItem>,
) {
    while let Some(bookmark) = node {
        if items.len() >= MAX_OUTLINE_ITEMS || depth > MAX_OUTLINE_DEPTH {
            return;
        }
        let page = target_page(bookmark.destination(), bookmark.action());
        items.push(OutlineItem {
            title: bookmark.title().unwrap_or_default().trim().to_owned(),
            depth,
            page,
        });
        walk_outline(bookmark.first_child(), depth + 1, items);
        node = bookmark.next_sibling();
    }
}

/// The page a bookmark or link points to, through a destination or a
/// go-to action.
pub(crate) fn target_page(
    destination: Option<PdfDestination>,
    action: Option<PdfAction>,
) -> Option<usize> {
    let page = match destination {
        Some(d) => d.page_index().ok()?,
        None => action?
            .as_local_destination_action()?
            .destination()
            .ok()?
            .page_index()
            .ok()?,
    };
    usize::try_from(page).ok()
}

pub(crate) fn link_at(doc: &PdfDocument, page: usize, (x, y): (f32, f32)) -> Option<LinkTarget> {
    let pdf_page = load_page(doc, page).ok()?;
    let (w, h) = (pdf_page.width().value, pdf_page.height().value);
    let links = pdf_page.links();
    let link = links.link_at_point(PdfPoints::new(x * w), PdfPoints::new(h - y * h))?;
    if let Some(page) = target_page(link.destination(), link.action()) {
        return Some(LinkTarget::Page(page));
    }
    let uri = link.action()?.as_uri_action()?.uri().ok()?;
    safe_uri(&uri).then_some(LinkTarget::Uri(uri))
}

/// Only web and mail links are opened; a PDF must not be able to launch
/// programs or local files.
pub(crate) fn safe_uri(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        && !uri.chars().any(|c| c.is_control() || c == '"')
}

/// Page labels (e.g. "iv"), one per page, empty when a page has none.
/// Read from a second handle on the document through the raw bindings:
/// `PdfPage::label` would load and parse every page (300 ms for 1000
/// pages), while the label tree lookup alone takes about a millisecond.
pub(crate) fn page_labels(
    pdfium: &Pdfium,
    path: &Path,
    password: Option<&str>,
    count: usize,
) -> Vec<String> {
    let Some(path) = path.to_str() else {
        return Vec::new();
    };
    let bindings = pdfium.bindings();
    // SAFETY: the handle is checked for null, used only within this block,
    // and closed exactly once. Each buffer passed to FPDF_GetPageLabel is
    // as long as the length PDFium just reported for that label.
    unsafe {
        let handle = bindings.FPDF_LoadDocument(path, password);
        if handle.is_null() {
            return Vec::new();
        }
        let labels = (0..count)
            .map(|page| {
                let Ok(index) = i32::try_from(page) else {
                    return String::new();
                };
                let len = bindings.FPDF_GetPageLabel(handle, index, std::ptr::null_mut(), 0);
                let Ok(size) = usize::try_from(len) else {
                    return String::new();
                };
                if size <= 2 {
                    return String::new();
                }
                let mut buffer = vec![0u8; size];
                bindings.FPDF_GetPageLabel(handle, index, buffer.as_mut_ptr().cast(), len);
                utf16le(&buffer)
            })
            .collect();
        bindings.FPDF_CloseDocument(handle);
        labels
    }
}

/// Decodes NUL-terminated UTF-16LE.
pub(crate) fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

pub(crate) fn load_page<'a>(doc: &'a PdfDocument, page: usize) -> Result<PdfPage<'a>, PdfiumError> {
    let index = PdfPageIndex::try_from(page).map_err(|_| PdfiumError::PageIndexOutOfBounds)?;
    doc.pages().get(index)
}

pub(crate) fn file_title(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned())
}
