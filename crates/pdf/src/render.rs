//! Rendering pages to pixels, and recoloring them.

use crate::*;

/// `forms` draws form fields; thumbnails skip them, too small to matter.
///
/// PDFium draws straight into our buffer as RGBx, which is then packed to
/// RGB and recolored in the same pass: one buffer and one pass per page.
pub(crate) fn render(
    doc: &PdfDocument,
    page: usize,
    width_px: u32,
    tint: PageTint,
    forms: bool,
) -> Result<Bitmap, PdfiumError> {
    let pdf_page = load_page(doc, page)?;
    let (w, h) = (pdf_page.width().value, pdf_page.height().value);
    let width = capped_width(width_px, (w, h));
    // As PDFium sizes it for a target width, so the page fills the buffer.
    let height = if w > 0.0 {
        ((h * (width as f32 / w)).round() as u32).max(1)
    } else {
        1
    };
    let (Ok(w_px), Ok(h_px)) = (Pixels::try_from(width), Pixels::try_from(height)) else {
        return Err(PdfiumError::ImageSizeOutOfBounds);
    };
    let pixels = width as usize * height as usize;
    let mut buffer = vec![0u8; pixels * 4];
    {
        let mut bitmap = PdfBitmap::from_bytes(w_px, h_px, PdfBitmapFormat::BGRx, &mut buffer)?;
        let config = PdfRenderConfig::new()
            .set_target_width(w_px)
            .set_reverse_byte_order(true)
            .render_form_data(forms);
        pdf_page.render_into_bitmap_with_config(&mut bitmap, &config)?;
    }
    pack_rgb(&mut buffer, &tint_lut(tint));
    Ok(Bitmap {
        width,
        height,
        rgb: buffer,
    })
}

/// One lookup table per channel: what each value becomes under `tint`.
pub(crate) fn tint_lut(tint: PageTint) -> [[u8; 256]; 3] {
    let mut luts = [[0u8; 256]; 3];
    for (channel, lut) in luts.iter_mut().enumerate() {
        for (i, out) in lut.iter_mut().enumerate() {
            *out = match tint {
                PageTint::None => i as u8,
                // Inverted lightness, with pure black softened so a page
                // reads as a dark-grey surface rather than a black hole.
                PageTint::Inverted => (24 + (255 - i) * 206 / 255) as u8,
                // Multiplied by the paper: white becomes the paper's color
                // and black stays black, so ink keeps its contrast.
                PageTint::Paper(paper) => ((i * usize::from(paper[channel]) + 127) / 255) as u8,
            };
        }
    }
    luts
}

/// Packs RGBx pixels to RGB in place through `luts`, and drops the rest.
pub(crate) fn pack_rgb(buffer: &mut Vec<u8>, luts: &[[u8; 256]; 3]) {
    let pixels = buffer.len() / 4;
    for i in 0..pixels {
        // Each pixel moves left, onto bytes already read.
        let [r, g, b, _] = [
            buffer[4 * i],
            buffer[4 * i + 1],
            buffer[4 * i + 2],
            buffer[4 * i + 3],
        ];
        buffer[3 * i] = luts[0][r as usize];
        buffer[3 * i + 1] = luts[1][g as usize];
        buffer[3 * i + 2] = luts[2][b as usize];
    }
    buffer.truncate(pixels * 3);
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
