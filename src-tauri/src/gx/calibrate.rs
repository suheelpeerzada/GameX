//! Lining up a captured level with the game's screen, without the user
//! calibrating the picker: objects whose sprite GameX knows (Jonny, Lucy...)
//! are looked for in the screenshot, and where they're found against where
//! the game says they are gives how game units map onto screen pixels.

/// An image as 8-bit RGBA.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// Reads a PNG (any colour type) as RGBA.
pub fn decode_png(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = w * h;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].as_chunks::<2>().0.iter().flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Image { w, h, rgba })
}

/// The image mirrored left to right (games often draw left-facing as a flipped right-facing frame).
pub fn mirrored(img: &Image) -> Image {
    let mut rgba = Vec::with_capacity(img.rgba.len());
    for y in 0..img.h {
        for x in (0..img.w).rev() {
            rgba.extend_from_slice(&img.rgba[(y * img.w + x) * 4..][..4]);
        }
    }
    Image { w: img.w, h: img.h, rgba }
}

/// An image shrunk to `w` x `h` by averaging (colour weighted by alpha), as RGB + alpha in 0..1.
struct Small {
    w: usize,
    h: usize,
    rgb: Vec<[f32; 3]>,
    alpha: Vec<f32>,
}

fn resample(img: &Image, w: usize, h: usize) -> Small {
    let (w, h) = (w.max(1), h.max(1));
    let mut rgb = vec![[0f32; 3]; w * h];
    let mut alpha = vec![0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y * img.h / h, ((y + 1) * img.h / h).max(y * img.h / h + 1).min(img.h));
        for x in 0..w {
            let (x0, x1) = (x * img.w / w, ((x + 1) * img.w / w).max(x * img.w / w + 1).min(img.w));
            let (mut c, mut a, mut n) = ([0f32; 3], 0f32, 0f32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = &img.rgba[(sy * img.w + sx) * 4..][..4];
                    let pa = p[3] as f32 / 255.0;
                    for k in 0..3 {
                        c[k] += p[k] as f32 * pa;
                    }
                    a += pa;
                    n += 1.0;
                }
            }
            let i = y * w + x;
            rgb[i] = if a > 0.0 { [c[0] / a, c[1] / a, c[2] / a] } else { [0.0; 3] };
            alpha[i] = if n > 0.0 { a / n } else { 0.0 };
        }
    }
    Small { w, h, rgb, alpha }
}

/// The best place for `t` in `s` (top-left), searched in the window given, and its score
/// (mean colour difference over the template's solid pixels; lower is better).
fn best_place(s: &Small, t: &Small, window: (usize, usize, usize, usize)) -> Option<(usize, usize, f32)> {
    if t.w > s.w || t.h > s.h {
        return None;
    }
    let solid: Vec<(usize, usize, [f32; 3])> = (0..t.h).flat_map(|y| (0..t.w).map(move |x| (x, y))).filter(|&(x, y)| t.alpha[y * t.w + x] > 0.6).map(|(x, y)| (x, y, t.rgb[y * t.w + x])).collect();
    // Too few solid pixels to tell anything apart.
    if solid.len() < 6 {
        return None;
    }
    let (x0, y0, x1, y1) = window;
    let mut best: Option<(usize, usize, f32)> = None;
    for y in y0..=y1.min(s.h - t.h) {
        for x in x0..=x1.min(s.w - t.w) {
            let mut sum = 0f32;
            let limit = best.map_or(f32::MAX, |b| b.2 * solid.len() as f32);
            for &(tx, ty, c) in &solid {
                let p = s.rgb[(y + ty) * s.w + x + tx];
                sum += (p[0] - c[0]).abs() + (p[1] - c[1]).abs() + (p[2] - c[2]).abs();
                if sum > limit {
                    break;
                }
            }
            let score = sum / solid.len() as f32;
            if best.is_none_or(|b| score < b.2) {
                best = Some((x, y, score));
            }
        }
    }
    best
}

/// Where `sprite` shows on `screen`: its middle in screen pixels, and how well it matched
/// (mean colour difference, 0..765; lower is better). Tried at sizes from half to six times.
pub fn find(screen: &Image, sprite: &Image) -> Option<(f64, f64, f32)> {
    // Coarse: the screen at about 160 pixels tall.
    let coarse = (screen.h / 160).max(1);
    let s = resample(screen, screen.w / coarse, screen.h / coarse);
    let mut scale = 0.5f64;
    let mut found: Option<(f64, usize, usize, f32)> = None;
    while scale <= 6.0 {
        let (tw, th) = (((sprite.w as f64 * scale) / coarse as f64).round() as usize, ((sprite.h as f64 * scale) / coarse as f64).round() as usize);
        if tw >= 3 && th >= 3 {
            let t = resample(sprite, tw, th);
            if let Some((x, y, score)) = best_place(&s, &t, (0, 0, s.w, s.h)) {
                if found.is_none_or(|f| score < f.3) {
                    found = Some((scale, x, y, score));
                }
            }
        }
        scale *= 1.15;
    }
    let (scale, cx, cy, _) = found?;
    // Fine: the screen at about 640 pixels tall, near the coarse place, at nearby sizes.
    let fine = (screen.h / 640).max(1);
    let f = resample(screen, screen.w / fine, screen.h / fine);
    let k = coarse / fine;
    let mut best: Option<(f64, f64, f32)> = None;
    for step in [0.88, 0.94, 1.0, 1.06, 1.12] {
        let sc = scale * step;
        let (tw, th) = (((sprite.w as f64 * sc) / fine as f64).round() as usize, ((sprite.h as f64 * sc) / fine as f64).round() as usize);
        if tw < 3 || th < 3 {
            continue;
        }
        let t = resample(sprite, tw, th);
        let window = ((cx * k).saturating_sub(2 * k), (cy * k).saturating_sub(2 * k), cx * k + 2 * k, cy * k + 2 * k);
        if let Some((x, y, score)) = best_place(&f, &t, window) {
            let middle = (((x as f64) + tw as f64 / 2.0) * fine as f64, ((y as f64) + th as f64 / 2.0) * fine as f64);
            if best.is_none_or(|b| score < b.2) {
                best = Some((middle.0, middle.1, score));
            }
        }
    }
    best
}

/// screen = a * game + b, one scale for both axes, from (game x, game y, screen x, screen y)
/// pairs. None without enough spread, or when the pairs don't agree with each other.
pub fn solve(points: &[(f64, f64, f64, f64)]) -> Option<[f64; 4]> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let m = |f: fn(&(f64, f64, f64, f64)) -> f64| points.iter().map(f).sum::<f64>() / n;
    let (gx, gy, sx, sy) = (m(|p| p.0), m(|p| p.1), m(|p| p.2), m(|p| p.3));
    let spread: f64 = points.iter().map(|p| (p.0 - gx).powi(2) + (p.1 - gy).powi(2)).sum();
    if spread < 1e-6 {
        return None;
    }
    let a = points.iter().map(|p| (p.0 - gx) * (p.2 - sx) + (p.1 - gy) * (p.3 - sy)).sum::<f64>() / spread;
    if a <= 1e-6 {
        return None;
    }
    let (bx, by) = (sx - a * gx, sy - a * gy);
    // Every pair should land near where the fit puts it.
    let worst = points.iter().map(|p| ((a * p.0 + bx - p.2).powi(2) + (a * p.1 + by - p.3).powi(2)).sqrt()).fold(0.0, f64::max);
    let size = (spread / n).sqrt() * a;
    (points.len() == 2 || worst <= (size * 0.25).max(24.0)).then_some([a, bx, a, by])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen of noise-free stripes with sprites drawn at known places and size.
    fn screen_with(sprites: &[(&Image, usize, usize, usize)], w: usize, h: usize) -> Image {
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let v = (((x / 7) + (y / 11)) % 3 * 40 + 30) as u8;
                rgba.extend_from_slice(&[v, v / 2, 60, 255]);
            }
        }
        let mut img = Image { w, h, rgba };
        for &(s, left, top, scale) in sprites {
            for y in 0..s.h * scale {
                for x in 0..s.w * scale {
                    let p = &s.rgba[((y / scale) * s.w + x / scale) * 4..][..4];
                    if p[3] > 128 {
                        let i = ((top + y) * w + left + x) * 4;
                        img.rgba[i..i + 4].copy_from_slice(p);
                    }
                }
            }
        }
        img
    }

    fn sprite(seed: u8) -> Image {
        let (w, h) = (12, 16);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let inside = (x as i32 - 6).pow(2) + (y as i32 - 8).pow(2) < 40;
                rgba.extend_from_slice(&[seed.wrapping_mul(37).wrapping_add((x * 9) as u8), 200 - (y * 7) as u8, seed.wrapping_mul(91), if inside { 255 } else { 0 }]);
            }
        }
        Image { w, h, rgba }
    }

    #[test]
    fn sprites_are_found_and_the_mapping_solved() {
        let (a, b) = (sprite(1), sprite(2));
        // Game units -> screen: x * 2 + 40, y * 2 + 100; sprites drawn 3x, centred on their object.
        let place = |gx: f64, gy: f64, s: &Image| ((gx * 2.0 + 40.0) as usize - s.w * 3 / 2, (gy * 2.0 + 100.0) as usize - s.h * 3 / 2);
        let (ax, ay) = place(60.0, 80.0, &a);
        let (bx, by) = place(250.0, 400.0, &b);
        let screen = screen_with(&[(&a, ax, ay, 3), (&b, bx, by, 3)], 720, 1280);
        let fa = find(&screen, &a).unwrap();
        let fb = find(&screen, &b).unwrap();
        assert!((fa.0 - 160.0).abs() < 8.0 && (fa.1 - 260.0).abs() < 8.0, "{fa:?}");
        assert!((fb.0 - 540.0).abs() < 8.0 && (fb.1 - 900.0).abs() < 8.0, "{fb:?}");
        let c = solve(&[(60.0, 80.0, fa.0, fa.1), (250.0, 400.0, fb.0, fb.1)]).unwrap();
        assert!((c[0] - 2.0).abs() < 0.05 && (c[1] - 40.0).abs() < 10.0 && (c[3] - 100.0).abs() < 10.0, "{c:?}");
    }

    #[test]
    fn disagreeing_pairs_are_refused() {
        assert!(solve(&[(0.0, 0.0, 0.0, 0.0), (100.0, 0.0, 200.0, 0.0), (0.0, 100.0, 900.0, 900.0)]).is_none());
        assert!(solve(&[(5.0, 5.0, 10.0, 10.0)]).is_none());
    }
}
