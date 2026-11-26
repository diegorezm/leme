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

fn render_raw_rgbx(
    pdf_page: &PdfPage,
    width: u32,
    height: u32,
    forms: bool,
) -> Result<Vec<u8>, PdfiumError> {
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
    Ok(buffer)
}

/// Renders a page with optional visual crop and clockwise rotation.
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
        let mut buffer = render_raw_rgbx(&pdf_page, width, height, forms)?;
        pack_rgb(&mut buffer, &tint_lut(tint));
        return Ok(Bitmap {
            width,
            height,
            rgb: buffer,
        });
    }

    let (disp_w, disp_h) = transform.display_size((w, h));
    let out_w = capped_width(width_px, (disp_w, disp_h));
    let out_h = if disp_w > 0.0 {
        ((disp_h * (out_w as f32 / disp_w)).round() as u32).max(1)
    } else {
        1
    };
    let crop_w_frac = transform
        .crop
        .filter(|c| c.width > 0.0 && c.height > 0.0)
        .map_or(1.0, |c| c.width);
    let desired_raw_w = if matches!(
        ViewTransform::normalize_rotation(transform.rotation),
        90 | 270
    ) {
        ((out_h as f32) / crop_w_frac).round() as u32
    } else {
        ((out_w as f32) / crop_w_frac).round() as u32
    };
    let raw_w = capped_width(desired_raw_w, (w, h));
    let raw_h = if w > 0.0 {
        ((h * (raw_w as f32 / w)).round() as u32).max(1)
    } else {
        1
    };
    let buffer = render_raw_rgbx(&pdf_page, raw_w, raw_h, forms)?;
    let (width, height, rgb) =
        pack_rgb_transformed(&buffer, raw_w, raw_h, &tint_lut(tint), transform);
    Ok(Bitmap { width, height, rgb })
}

/// Crops, rotates, and packs an RGBx buffer into tightly packed RGB bytes.
pub(crate) fn pack_rgb_transformed(
    buffer: &[u8],
    raw_w: u32,
    raw_h: u32,
    luts: &[[u8; 256]; 3],
    transform: ViewTransform,
) -> (u32, u32, Vec<u8>) {
    let rw = raw_w.max(1);
    let rh = raw_h.max(1);
    let (x0, y0, cw, ch) = match transform.crop {
        Some(c) if c.width > 0.0 && c.height > 0.0 => {
            let x0 = (c.x * rw as f32).floor().clamp(0.0, (rw - 1) as f32) as u32;
            let y0 = (c.y * rh as f32).floor().clamp(0.0, (rh - 1) as f32) as u32;
            let x1 = ((c.x + c.width) * rw as f32)
                .ceil()
                .clamp((x0 + 1) as f32, rw as f32) as u32;
            let y1 = ((c.y + c.height) * rh as f32)
                .ceil()
                .clamp((y0 + 1) as f32, rh as f32) as u32;
            (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        }
        _ => (0, 0, rw, rh),
    };
    let rot = ViewTransform::normalize_rotation(transform.rotation);
    let (out_w, out_h) = if matches!(rot, 90 | 270) {
        (ch, cw)
    } else {
        (cw, ch)
    };
    let mut rgb = vec![0u8; out_w as usize * out_h as usize * 3];
    for dy in 0..out_h {
        for dx in 0..out_w {
            let (sx, sy) = match rot {
                90 => (x0 + dy, y0 + ch - 1 - dx),
                180 => (x0 + cw - 1 - dx, y0 + ch - 1 - dy),
                270 => (x0 + cw - 1 - dy, y0 + dx),
                _ => (x0 + dx, y0 + dy),
            };
            let src = (sy as usize * rw as usize + sx as usize) * 4;
            let dst = (dy as usize * out_w as usize + dx as usize) * 3;
            rgb[dst] = luts[0][buffer[src] as usize];
            rgb[dst + 1] = luts[1][buffer[src + 1] as usize];
            rgb[dst + 2] = luts[2][buffer[src + 2] as usize];
        }
    }
    (out_w, out_h, rgb)
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
        if bmp.width == 0 || bmp.height == 0 {
            continue;
        }
        let wf = bmp.width as f32;
        let hf = bmp.height as f32;
        for y in 0..bmp.height {
            for x in 0..bmp.width {
                let idx = (y as usize * bmp.width as usize + x as usize) * 3;
                let [r, g, b] = [bmp.rgb[idx], bmp.rgb[idx + 1], bmp.rgb[idx + 2]];
                if r < 245 || g < 245 || b < 245 {
                    min_x = min_x.min(x as f32 / wf);
                    min_y = min_y.min(y as f32 / hf);
                    max_x = max_x.max((x + 1) as f32 / wf);
                    max_y = max_y.max((y + 1) as f32 / hf);
                }
            }
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
