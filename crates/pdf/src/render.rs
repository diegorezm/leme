//! Rendering pages to pixels, and recoloring them.

use crate::*;

use domain::geometry::ViewTransform;

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
    render_transformed(doc, page, width_px, tint, forms, ViewTransform::default())
}

/// Computes `(out_w, out_h, full_w, full_h, start_x, start_y, rotation)` for
/// rendering a page with optional crop and clockwise rotation directly into an
/// `out_w x out_h` bitmap via PDFium.
pub(crate) fn render_geometry(
    (w, h): (f32, f32),
    width_px: u32,
    transform: ViewTransform,
) -> (u32, u32, i32, i32, i32, i32, u16) {
    let rot = ViewTransform::normalize_rotation(transform.rotation);
    let (disp_w, disp_h) = transform.display_size((w, h));
    let out_w = capped_width(width_px, (disp_w, disp_h));
    let out_h = if disp_w > 0.0 {
        ((disp_h * (out_w as f32 / disp_w)).round() as u32).max(1)
    } else {
        1
    };
    let (cx, cy, cw, ch) = match transform.crop {
        Some(c) if c.width > 0.0 && c.height > 0.0 => (
            c.x.clamp(0.0, 1.0),
            c.y.clamp(0.0, 1.0),
            c.width.clamp(0.01, 1.0),
            c.height.clamp(0.01, 1.0),
        ),
        _ => (0.0, 0.0, 1.0, 1.0),
    };
    // Crop rect mapped into rotated full-page normalized coordinates.
    let (vx, vy, vw, vh) = match rot {
        90 => ((1.0 - (cy + ch)).max(0.0), cx, ch, cw),
        180 => (
            (1.0 - (cx + cw)).max(0.0),
            (1.0 - (cy + ch)).max(0.0),
            cw,
            ch,
        ),
        270 => (cy, (1.0 - (cx + cw)).max(0.0), ch, cw),
        _ => (cx, cy, cw, ch),
    };
    let full_w = ((out_w as f32 / vw).round() as i32).max(out_w as i32);
    let full_h = ((out_h as f32 / vh).round() as i32).max(out_h as i32);
    let off_x = ((vx * full_w as f32).round() as i32).clamp(0, full_w - out_w as i32);
    let off_y = ((vy * full_h as f32).round() as i32).clamp(0, full_h - out_h as i32);
    (out_w, out_h, full_w, full_h, -off_x, -off_y, rot)
}

/// Renders a page with optional visual crop and clockwise rotation directly
/// into a single `out_w x out_h` buffer via PDFium, then packs to RGB in place.
pub(crate) fn render_transformed(
    doc: &PdfDocument,
    page: usize,
    width_px: u32,
    tint: PageTint,
    forms: bool,
    transform: ViewTransform,
) -> Result<Bitmap, PdfiumError> {
    let pdf_page = load_page(doc, page)?;
    let (w, h) = (pdf_page.width().value, pdf_page.height().value);
    if transform.is_identity() {
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
        let mut buffer = vec![0u8; width as usize * height as usize * 4];
        {
            let mut bitmap = PdfBitmap::from_bytes(w_px, h_px, PdfBitmapFormat::BGRx, &mut buffer)?;
            let config = PdfRenderConfig::new()
                .set_target_width(w_px)
                .set_reverse_byte_order(true)
                .render_form_data(forms);
            pdf_page.render_into_bitmap_with_config(&mut bitmap, &config)?;
        }
        pack_rgb(&mut buffer, &tint_lut(tint));
        return Ok(Bitmap {
            width,
            height,
            rgb: buffer,
        });
    }

    let (out_w, out_h, full_w, full_h, start_x, start_y, rot) =
        render_geometry((w, h), width_px, transform);
    let (Ok(w_px), Ok(h_px)) = (Pixels::try_from(out_w), Pixels::try_from(out_h)) else {
        return Err(PdfiumError::ImageSizeOutOfBounds);
    };
    let mut buffer = vec![255u8; out_w as usize * out_h as usize * 4];
    {
        let mut bitmap = PdfBitmap::from_bytes(w_px, h_px, PdfBitmapFormat::BGRx, &mut buffer)?;
        let pdf_rot = match rot {
            90 => PdfPageRenderRotation::Degrees90,
            180 => PdfPageRenderRotation::Degrees180,
            270 => PdfPageRenderRotation::Degrees270,
            _ => PdfPageRenderRotation::None,
        };
        // `FPDF_RenderPageBitmap` (used when `render_form_data(true)`) applies
        // `start_x`/`start_y`, `fixed_size`, and `rotate` natively and clips
        // cropped margins before rasterizing.
        let config = PdfRenderConfig::new()
            .set_fixed_size(full_w, full_h)
            .set_origin(start_x, start_y)
            .rotate(pdf_rot, false)
            .set_reverse_byte_order(true)
            .render_form_data(true);
        pdf_page.render_into_bitmap_with_config(&mut bitmap, &config)?;
    }
    pack_rgb(&mut buffer, &tint_lut(tint));
    Ok(Bitmap {
        width: out_w,
        height: out_h,
        rgb: buffer,
    })
}

/// Estimates a union content bounding box across sampled pages of `doc` so
/// empty white margins can be visually cropped without hiding content on any page.
pub(crate) fn detect_crop(
    doc: &PdfDocument,
    page_count: usize,
    current_page: usize,
) -> Option<NormRect> {
    if page_count == 0 {
        return None;
    }
    let mut samples = Vec::new();
    samples.push(0);
    if current_page < page_count && !samples.contains(&current_page) {
        samples.push(current_page);
    }
    let target = 12usize.min(page_count);
    for i in 0..target {
        let p = (i * page_count) / target;
        if !samples.contains(&p) {
            samples.push(p);
        }
    }

    let mut min_x = 1.0_f32;
    let mut min_y = 1.0_f32;
    let mut max_x = 0.0_f32;
    let mut max_y = 0.0_f32;
    for page in samples {
        let Ok(bmp) = render(doc, page, 160, PageTint::None, false) else {
            continue;
        };
        let bw = bmp.width as usize;
        let bh = bmp.height as usize;
        if bw == 0 || bh == 0 {
            continue;
        }
        let mut p_min_x = bw;
        let mut p_min_y = bh;
        let mut p_max_x = 0usize;
        let mut p_max_y = 0usize;
        let row_bytes = bw * 3;
        for (y, row) in bmp.rgb.chunks_exact(row_bytes).enumerate() {
            let mut row_hit = false;
            for (x, &[r, g, b]) in row.as_chunks::<3>().0.iter().enumerate() {
                if r < 245 || g < 245 || b < 245 {
                    row_hit = true;
                    if x < p_min_x {
                        p_min_x = x;
                    }
                    if x + 1 > p_max_x {
                        p_max_x = x + 1;
                    }
                }
            }
            if row_hit {
                if y < p_min_y {
                    p_min_y = y;
                }
                p_max_y = y + 1;
            }
        }
        if p_max_x > p_min_x && p_max_y > p_min_y {
            let wf = bw as f32;
            let hf = bh as f32;
            min_x = min_x.min(p_min_x as f32 / wf);
            min_y = min_y.min(p_min_y as f32 / hf);
            max_x = max_x.max(p_max_x as f32 / wf);
            max_y = max_y.max(p_max_y as f32 / hf);
        }
    }
    if max_x <= min_x || max_y <= min_y {
        return None;
    }
    let pad = 0.02_f32;
    let x = (min_x - pad).clamp(0.0, 1.0);
    let y = (min_y - pad).clamp(0.0, 1.0);
    let x1 = (max_x + pad).clamp(0.0, 1.0);
    let y1 = (max_y + pad).clamp(0.0, 1.0);
    let width = x1 - x;
    let height = y1 - y;
    if width < 0.10 || height < 0.10 || (width >= 0.96 && height >= 0.96) {
        return None;
    }
    Some(NormRect {
        x,
        y,
        width,
        height,
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
