//! Shapes shared by the reader, annotations, and the PDF worker.

use serde::{Deserialize, Serialize};

/// A rectangle as fractions of the page size, origin at the top left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Visual page transform (non-destructive rotation and margin crop).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewTransform {
    /// Clockwise rotation in degrees: 0, 90, 180, or 270.
    pub rotation: u16,
    /// Optional content region in unrotated normalized `[0..1]` page coordinates.
    pub crop: Option<NormRect>,
}

impl ViewTransform {
    pub fn normalize_rotation(degrees: u16) -> u16 {
        match (degrees / 90) % 4 {
            1 => 90,
            2 => 180,
            3 => 270,
            _ => 0,
        }
    }

    pub fn is_identity(self) -> bool {
        Self::normalize_rotation(self.rotation) == 0 && self.crop.is_none()
    }

    /// Page dimensions in points after cropping and rotating.
    pub fn display_size(self, (w, h): (f32, f32)) -> (f32, f32) {
        let (cw, ch) = match self.crop {
            Some(c) if c.width > 0.0 && c.height > 0.0 => (w * c.width, h * c.height),
            _ => (w, h),
        };
        if matches!(Self::normalize_rotation(self.rotation), 90 | 270) {
            (ch, cw)
        } else {
            (cw, ch)
        }
    }

    /// Maps an unrotated, uncropped document `NormRect` into view `[0..1]` fractions.
    /// Returns `None` if completely outside the cropped region.
    pub fn to_view(self, rect: NormRect) -> Option<NormRect> {
        let cropped = match self.crop {
            Some(c) if c.width > 0.0 && c.height > 0.0 => {
                let x0 = ((rect.x - c.x) / c.width).clamp(0.0, 1.0);
                let y0 = ((rect.y - c.y) / c.height).clamp(0.0, 1.0);
                let x1 = ((rect.x + rect.width - c.x) / c.width).clamp(0.0, 1.0);
                let y1 = ((rect.y + rect.height - c.y) / c.height).clamp(0.0, 1.0);
                let w = x1 - x0;
                let h = y1 - y0;
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                NormRect {
                    x: x0,
                    y: y0,
                    width: w,
                    height: h,
                }
            }
            _ => rect,
        };
        Some(match Self::normalize_rotation(self.rotation) {
            90 => NormRect {
                x: (1.0 - (cropped.y + cropped.height)).clamp(0.0, 1.0),
                y: cropped.x,
                width: cropped.height,
                height: cropped.width,
            },
            180 => NormRect {
                x: (1.0 - (cropped.x + cropped.width)).clamp(0.0, 1.0),
                y: (1.0 - (cropped.y + cropped.height)).clamp(0.0, 1.0),
                width: cropped.width,
                height: cropped.height,
            },
            270 => NormRect {
                x: cropped.y,
                y: (1.0 - (cropped.x + cropped.width)).clamp(0.0, 1.0),
                width: cropped.height,
                height: cropped.width,
            },
            _ => cropped,
        })
    }

    /// Maps a point in view `[0..1]` fractions back to unrotated, uncropped document fractions.
    pub fn point_to_doc(self, (vx, vy): (f32, f32)) -> (f32, f32) {
        let (ux, uy) = match Self::normalize_rotation(self.rotation) {
            90 => (vy, 1.0 - vx),
            180 => (1.0 - vx, 1.0 - vy),
            270 => (1.0 - vy, vx),
            _ => (vx, vy),
        };
        match self.crop {
            Some(c) if c.width > 0.0 && c.height > 0.0 => (c.x + ux * c.width, c.y + uy * c.height),
            _ => (ux, uy),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_round_trips_points_and_rects() {
        let crop = NormRect {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        };
        let rect = NormRect {
            x: 0.2,
            y: 0.3,
            width: 0.2,
            height: 0.1,
        };
        for rot in [0, 90, 180, 270] {
            let vt = ViewTransform {
                rotation: rot,
                crop: Some(crop),
            };
            let v = vt.to_view(rect).unwrap();
            let center_view = (v.x + v.width / 2.0, v.y + v.height / 2.0);
            let (dx, dy) = vt.point_to_doc(center_view);
            assert!((dx - 0.3).abs() < 1e-4, "rot={rot} dx={dx}");
            assert!((dy - 0.35).abs() < 1e-4, "rot={rot} dy={dy}");
        }
        let outside = NormRect {
            x: 0.0,
            y: 0.0,
            width: 0.05,
            height: 0.05,
        };
        assert!(ViewTransform {
            rotation: 90,
            crop: Some(crop),
        }
        .to_view(outside)
        .is_none());
    }
}
