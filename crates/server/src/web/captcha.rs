//! Java: `util/Captcha`: the registration captcha text and a 200x50 PNG of it (random lines and
//! rotated, scaled characters). Java draws Arial with Java2D; this draws a 5x7 bitmap font the
//! same way, so the images are alike but not the same bytes (they are random in Java anyway).

use rand::RngExt;

const W: usize = 200;
const H: usize = 50;

/// Java: `Captcha.generateText(length)`: `length - 1` random letters and digits, lowercased.
pub fn generate_text(length: usize) -> String {
    const DATA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..length.saturating_sub(1))
        .map(|_| DATA[rng.random_range(0..DATA.len())] as char)
        .collect::<String>()
        .to_lowercase()
}

/// 5x7 glyphs, one byte per row (low 5 bits, MSB on the left).
fn glyph(c: char) -> [u8; 7] {
    match c {
        'a' => [0x00, 0x00, 0x0e, 0x01, 0x0f, 0x11, 0x0f],
        'b' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x1e],
        'c' => [0x00, 0x00, 0x0e, 0x10, 0x10, 0x11, 0x0e],
        'd' => [0x01, 0x01, 0x0d, 0x13, 0x11, 0x11, 0x0f],
        'e' => [0x00, 0x00, 0x0e, 0x11, 0x1f, 0x10, 0x0e],
        'f' => [0x06, 0x09, 0x08, 0x1c, 0x08, 0x08, 0x08],
        'g' => [0x00, 0x0f, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'h' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11],
        'i' => [0x04, 0x00, 0x0c, 0x04, 0x04, 0x04, 0x0e],
        'j' => [0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0c],
        'k' => [0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12],
        'l' => [0x0c, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'm' => [0x00, 0x00, 0x1a, 0x15, 0x15, 0x11, 0x11],
        'n' => [0x00, 0x00, 0x16, 0x19, 0x11, 0x11, 0x11],
        'o' => [0x00, 0x00, 0x0e, 0x11, 0x11, 0x11, 0x0e],
        'p' => [0x00, 0x00, 0x1e, 0x11, 0x1e, 0x10, 0x10],
        'q' => [0x00, 0x00, 0x0d, 0x13, 0x0f, 0x01, 0x01],
        'r' => [0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10],
        's' => [0x00, 0x00, 0x0e, 0x10, 0x0e, 0x01, 0x1e],
        't' => [0x08, 0x08, 0x1c, 0x08, 0x08, 0x09, 0x06],
        'u' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0d],
        'v' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'w' => [0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0a],
        'x' => [0x00, 0x00, 0x11, 0x0a, 0x04, 0x0a, 0x11],
        'y' => [0x00, 0x00, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'z' => [0x00, 0x00, 0x1f, 0x02, 0x04, 0x08, 0x1f],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        _ => [0; 7],
    }
}

fn line(px: &mut [bool], (x0, y0): (i64, i64), (x1, y1): (i64, i64)) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        if (0..W as i64).contains(&x) && (0..H as i64).contains(&y) {
            px[y as usize * W + x as usize] = true;
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Java: `Captcha.generateImage`.
pub fn image(text: &str) -> Vec<u8> {
    let mut rng = rand::rng();
    let mut px = vec![false; W * H];
    for _ in 0..8 {
        let a = (rng.random_range(0..W) as i64, rng.random_range(0..H) as i64);
        let b = (rng.random_range(0..H) as i64, rng.random_range(0..H) as i64);
        line(&mut px, a, b);
    }
    let n = text.chars().count().max(1);
    let mut x_pos = 20i64;
    for ch in text.chars() {
        let box_width = (W / n) as i64 - rng.random_range(0..20);
        let degree: f64 = rng.random::<f64>() * 30.0 - 15.0;
        let scale = (1.0 - rng.random::<f64>() * 15.0 / 100.0) * 5.0;
        let bold = rng.random::<bool>();
        let (sin, cos) = (degree * std::f64::consts::PI / 90.0).sin_cos();
        let (cx, cy) = ((x_pos + box_width / 2) as f64, (H / 2) as f64);
        let g = glyph(ch);
        // Inverse mapping: each pixel near the character, rotated back into glyph space.
        for y in 0..H {
            for x in 0..W {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let gx = (cos * dx + sin * dy) / scale + 2.5;
                let gy = (-sin * dx + cos * dy) / scale + 3.5;
                let hit = |gx: f64, gy: f64| {
                    gx >= 0.0
                        && gy >= 0.0
                        && (gx as usize) < 5
                        && (gy as usize) < 7
                        && g[gy as usize] & (0x10 >> gx as usize) != 0
                };
                if hit(gx, gy) || (bold && hit(gx - 0.35, gy)) {
                    px[y * W + x] = true;
                }
            }
        }
        x_pos += box_width;
    }
    let mut rgb = Vec::with_capacity(W * H * 3);
    for p in px {
        let v = if p { 0 } else { 255 };
        rgb.extend_from_slice(&[v, v, v]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, W as u32, H as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(&rgb);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_image() {
        let t = generate_text(7);
        assert_eq!(t.len(), 6);
        assert!(
            t.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        let png = image(&t);
        assert_eq!(&png[1..4], b"PNG");
    }
}
