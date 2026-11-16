//! Rendering pages to pixels, and recoloring them.

use crate::*;

/// `forms` draws form fields; thumbnails skip them, too small to matter.
pub(crate) fn render(
    doc: &PdfDocument,
    page: usize,
    width_px: u32,
    tint: PageTint,
    forms: bool,
) -> Result<Bitmap, PdfiumError> {
    let pdf_page = load_page(doc, page)?;
    let size = (pdf_page.width().value, pdf_page.height().value);
    let width_px = capped_width(width_px, size);
    let width = Pixels::try_from(width_px).unwrap_or(Pixels::MAX);
    let config = PdfRenderConfig::new()
        .set_target_width(width)
        .render_form_data(forms);
    let bitmap = pdf_page.render_with_config(&config)?;
    let mut rgba = bitmap.as_rgba_bytes();
    match tint {
        PageTint::None => {}
        PageTint::Inverted => invert(&mut rgba),
        PageTint::Paper(paper) => tint_paper(&mut rgba, paper),
    }
    Ok(Bitmap {
        width: u32::try_from(bitmap.width()).unwrap_or(0),
        height: u32::try_from(bitmap.height()).unwrap_or(0),
        rgba,
    })
}

/// Most pixels one render may have: 24 M is 72 MB as RGB, an A4 page
/// about 4100 px wide. Deeper zoom stretches the render instead.
pub(crate) const MAX_RENDER_PIXELS: f32 = 24_000_000.0;
/// Longest side of a render; larger textures fail on many GPUs.
pub(crate) const MAX_RENDER_SIDE: f32 = 16_384.0;

/// `width_px`, shrunk so a page of `size` (points) stays within
/// `MAX_RENDER_PIXELS` and `MAX_RENDER_SIDE`. Never below one pixel.
pub(crate) fn capped_width(width_px: u32, (w, h): (f32, f32)) -> u32 {
    let width_px = width_px.max(1);
    if !(w > 0.0 && h > 0.0) {
        return width_px;
    }
    let max = (MAX_RENDER_PIXELS * w / h)
        .sqrt()
        .min(MAX_RENDER_SIDE)
        .min(MAX_RENDER_SIDE * w / h)
        .floor()
        .max(1.0) as u32;
    width_px.min(max)
}

/// Inverts RGB, leaving alpha, then softens pure black so a page reads as
/// a dark-grey surface rather than a black hole.
pub(crate) fn invert(rgba: &mut [u8]) {
    // A lookup table: one load per channel instead of a multiply and divide.
    const LUT: [u8; 256] = {
        let mut lut = [0; 256];
        let mut i = 0;
        while i < 256 {
            lut[i] = 24 + ((255 - i) * 206 / 255) as u8;
            i += 1;
        }
        lut
    };
    for px in rgba.as_chunks_mut::<4>().0 {
        for c in &mut px[..3] {
            *c = LUT[*c as usize];
        }
    }
}

/// Multiplies RGB by `paper`, leaving alpha: white becomes the paper's
/// color and black stays black, so ink keeps its contrast.
pub(crate) fn tint_paper(rgba: &mut [u8], paper: [u8; 3]) {
    // One table per channel: a load instead of a multiply and divide.
    let luts = paper.map(|p| {
        let mut lut = [0u8; 256];
        for (i, out) in lut.iter_mut().enumerate() {
            *out = ((i * usize::from(p) + 127) / 255) as u8;
        }
        lut
    });
    for px in rgba.as_chunks_mut::<4>().0 {
        for (c, lut) in px[..3].iter_mut().zip(&luts) {
            *c = lut[*c as usize];
        }
    }
}
