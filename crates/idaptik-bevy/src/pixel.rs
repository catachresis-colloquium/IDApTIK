//! Pixel-art building blocks rendered in code at the sprites' art density
//! ([`SPRITE_PX`]): a bitmap font for signs, stepped light cones, stepped
//! ellipses for floor shadows, and the plain repeating tiles (floor, ceiling
//! trim, door frames) whose pixels are placed exactly.
//!
//! Everything here makes an [`Image`] sampled nearest-neighbour, and a
//! [`Painted`] that draws it at its true size.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::sprites::{Painted, SPRITE_PX};

/// An RGBA8 pixel buffer, `w` x `h`, rows top-down.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            data: vec![0; w * h * 4],
        }
    }

    /// Set one pixel (ignored off the canvas).
    pub fn set(&mut self, x: i64, y: i64, rgba: [u8; 4]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 4;
            self.data[i..i + 4].copy_from_slice(&rgba);
        }
    }

    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Fill a rectangle.
    pub fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, rgba: [u8; 4]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, rgba);
            }
        }
    }

    /// Into a nearest-sampled image, drawn at its true size.
    pub fn into_painted(self, images: &mut Assets<Image>) -> Painted {
        let (w, h) = (self.w as u32, self.h as u32);
        let mut image = Image::new(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        Painted {
            image: images.add(image),
            size: Vec2::new(w as f32, h as f32) / SPRITE_PX,
        }
    }
}

/// A colour as RGBA8 with alpha `a` (0-1).
pub fn rgba(color: Color, a: f32) -> [u8; 4] {
    let s = color.to_srgba();
    [
        (s.red * 255.0).round() as u8,
        (s.green * 255.0).round() as u8,
        (s.blue * 255.0).round() as u8,
        (a * 255.0).round() as u8,
    ]
}

//## Bitmap font

/// Glyph cell: 5 x 7 pixels, one pixel between letters.
pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

/// The 5 x 7 glyph for a character (`#` is ink). Unknown characters draw as a
/// blank. Upper case only: lower case is drawn as upper.
fn glyph(c: char) -> [&'static str; 7] {
    match c.to_ascii_uppercase() {
        'A' => [
            ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
        'B' => [
            "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.",
        ],
        'C' => [
            ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
        ],
        'D' => [
            "####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####.",
        ],
        'E' => [
            "#####", "#....", "#....", "####.", "#....", "#....", "#####",
        ],
        'F' => [
            "#####", "#....", "#....", "####.", "#....", "#....", "#....",
        ],
        'G' => [
            ".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####",
        ],
        'H' => [
            "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
        ],
        'I' => [
            ".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
        'J' => [
            "..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##..",
        ],
        'K' => [
            "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
        ],
        'L' => [
            "#....", "#....", "#....", "#....", "#....", "#....", "#####",
        ],
        'M' => [
            "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#",
        ],
        'N' => [
            "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#",
        ],
        'O' => [
            ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
        'P' => [
            "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
        ],
        'Q' => [
            ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#",
        ],
        'R' => [
            "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#",
        ],
        'S' => [
            ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
        ],
        'T' => [
            "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..",
        ],
        'U' => [
            "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.",
        ],
        'V' => [
            "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..",
        ],
        'W' => [
            "#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#.",
        ],
        'X' => [
            "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
        ],
        'Y' => [
            "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..",
        ],
        'Z' => [
            "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
        ],
        '0' => [
            ".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###.",
        ],
        '1' => [
            "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.",
        ],
        '2' => [
            ".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####",
        ],
        '3' => [
            "####.", "....#", "....#", ".###.", "....#", "....#", "####.",
        ],
        '4' => [
            "...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.",
        ],
        '5' => [
            "#####", "#....", "####.", "....#", "....#", "#...#", ".###.",
        ],
        '6' => [
            ".###.", "#....", "#....", "####.", "#...#", "#...#", ".###.",
        ],
        '7' => [
            "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...",
        ],
        '8' => [
            ".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###.",
        ],
        '9' => [
            ".###.", "#...#", "#...#", ".####", "....#", "....#", ".###.",
        ],
        '-' => [
            ".....", ".....", ".....", ".###.", ".....", ".....", ".....",
        ],
        '/' => [
            "....#", "....#", "...#.", "..#..", ".#...", "#....", "#....",
        ],
        '.' => [
            ".....", ".....", ".....", ".....", ".....", ".##..", ".##..",
        ],
        ':' => [
            ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".....",
        ],
        '!' => [
            "..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#..",
        ],
        _ => [
            ".....", ".....", ".....", ".....", ".....", ".....", ".....",
        ],
    }
}

/// Render a line of text as pixel art: glyphs in `ink`, an optional one-pixel
/// drop shadow down and right in `shadow`, on transparent.
pub fn text_canvas(text: &str, ink: [u8; 4], shadow: Option<[u8; 4]>) -> Canvas {
    let n = text.chars().count();
    let pad = usize::from(shadow.is_some());
    let w = (n * (GLYPH_W + 1)).saturating_sub(1).max(1) + pad;
    let mut c = Canvas::new(w, GLYPH_H + pad);
    for pass in [shadow.map(|s| (1, s)), Some((0, ink))]
        .into_iter()
        .flatten()
    {
        let (off, color) = pass;
        for (k, ch) in text.chars().enumerate() {
            for (y, row) in glyph(ch).iter().enumerate() {
                for (x, px) in row.bytes().enumerate() {
                    if px == b'#' {
                        let gx = (k * (GLYPH_W + 1) + x + off) as i64;
                        c.set(gx, (y + off) as i64, color);
                    }
                }
            }
        }
    }
    c
}

//## Light and shadow

/// A ceiling light's cone: a trapezoid from `top_w` at the fitting to
/// `bottom_w` at the floor, `h` tall, in three flat bands of falling alpha
/// (brightest in the middle), the way pixel art draws light.
pub fn light_cone(top_w: usize, bottom_w: usize, h: usize, color: Color, alpha: f32) -> Canvas {
    let w = bottom_w.max(top_w);
    let mut c = Canvas::new(w, h);
    let mid = w as f32 * 0.5;
    for y in 0..h {
        let t = y as f32 / (h.max(2) - 1) as f32;
        let half = (top_w as f32 + (bottom_w as f32 - top_w as f32) * t) * 0.5;
        for x in 0..w {
            let d = ((x as f32 + 0.5) - mid).abs() / half.max(1.0);
            let band = if d > 1.0 {
                continue;
            } else if d > 0.72 {
                0.45
            } else if d > 0.4 {
                0.75
            } else {
                1.0
            };
            // Fade out over the bottom third, in two steps.
            let fade = if t > 0.86 {
                0.5
            } else if t > 0.66 {
                0.75
            } else {
                1.0
            };
            c.set(x as i64, y as i64, rgba(color, alpha * band * fade));
        }
    }
    c
}

/// A floor shadow: a flat ellipse, `w` x `h` pixels, with a lighter rim.
pub fn shadow_ellipse(w: usize, h: usize, color: Color, alpha: f32) -> Canvas {
    let mut c = Canvas::new(w, h);
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 + 0.5 - cx) / cx;
            let dy = (y as f32 + 0.5 - cy) / cy;
            let r = dx * dx + dy * dy;
            if r <= 1.0 {
                let a = if r > 0.55 { alpha * 0.55 } else { alpha };
                c.set(x as i64, y as i64, rgba(color, a));
            }
        }
    }
    c
}

//## Plain tiles

/// The floor: a worn linoleum band (`h` px) with a light top edge and a seam
/// every `seam` px; repeats sideways.
pub fn floor_tile(seam: usize, h: usize, base: Color, edge: Color, line: Color) -> Canvas {
    let mut c = Canvas::new(seam, h);
    c.rect(0, 0, seam as i64, h as i64, rgba(base, 1.0));
    c.rect(0, 0, seam as i64, 1, rgba(edge, 1.0));
    c.rect(seam as i64 - 1, 1, 1, h as i64 - 1, rgba(line, 1.0));
    c
}

/// The ceiling trim: a plain band with a lighter lower lip and a dark
/// ventilation slot every `pitch` px; repeats sideways.
pub fn ceiling_tile(pitch: usize, h: usize, base: Color, lip: Color, slot: Color) -> Canvas {
    let mut c = Canvas::new(pitch, h);
    c.rect(0, 0, pitch as i64, h as i64, rgba(base, 1.0));
    c.rect(0, h as i64 - 1, pitch as i64, 1, rgba(lip, 1.0));
    c.rect(
        pitch as i64 / 2 - 2,
        h as i64 / 2 - 1,
        4,
        2,
        rgba(slot, 1.0),
    );
    c
}

/// A door frame: two jambs and a head round an opening `door_w` x `door_h` px,
/// `t` px thick, with a light inner edge; the opening is transparent.
pub fn door_frame(door_w: usize, door_h: usize, t: usize, frame: Color, edge: Color) -> Canvas {
    let (w, h) = (door_w + 2 * t, door_h + t);
    let mut c = Canvas::new(w, h);
    let (f, e) = (rgba(frame, 1.0), rgba(edge, 1.0));
    c.rect(0, 0, w as i64, t as i64, f);
    c.rect(0, 0, t as i64, h as i64, f);
    c.rect((w - t) as i64, 0, t as i64, h as i64, f);
    // Light inner edge on the jambs and under the head.
    c.rect(t as i64 - 1, t as i64, 1, door_h as i64, e);
    c.rect((w - t) as i64, t as i64, 1, door_h as i64, e);
    c.rect(t as i64 - 1, t as i64 - 1, door_w as i64 + 2, 1, e);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_one_glyph_cell_per_character_with_a_one_pixel_gap() {
        let ink = [255, 255, 255, 255];
        let c = text_canvas("HI", ink, None);
        assert_eq!((c.w, c.h), (2 * GLYPH_W + 1, GLYPH_H));
        // H's left stem, the gap column, and I's top bar.
        assert_eq!(c.get(0, 3), ink);
        assert_eq!(c.get(GLYPH_W, 3)[3], 0);
        assert_eq!(c.get(GLYPH_W + 2, 0), ink);
        // A shadow adds one pixel each way.
        let s = text_canvas("HI", ink, Some([0, 0, 0, 255]));
        assert_eq!((s.w, s.h), (2 * GLYPH_W + 2, GLYPH_H + 1));
    }

    #[test]
    fn every_sign_character_has_a_glyph() {
        for ch in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-/.:!".chars() {
            assert!(glyph(ch).iter().any(|r| r.contains('#')), "{ch}");
            assert!(glyph(ch).iter().all(|r| r.len() == GLYPH_W), "{ch}");
        }
    }

    #[test]
    fn light_is_brightest_in_the_middle_and_stepped() {
        let c = light_cone(10, 40, 30, Color::WHITE, 0.2);
        let a = |x, y| c.get(x, y)[3];
        assert!(a(20, 10) > a(9, 25), "middle brighter than the edge");
        let alphas: std::collections::BTreeSet<u8> =
            (0..c.w * c.h).map(|i| c.data[i * 4 + 3]).collect();
        assert!(
            alphas.len() <= 10,
            "a handful of flat bands, not a gradient: {alphas:?}"
        );
    }
}
