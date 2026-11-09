//! `body = "image"`: PNG and GIF bodies compared by their decoded pixels, since Java's ImageIO
//! and the Rust encoders compress the same image differently. Fully transparent pixels compare
//! as equal whatever their colour.

/// `"png 64x110, pixels fnv …"`, or a note when the body is not an image this can decode.
pub fn describe(bytes: &[u8]) -> String {
    let decoded = if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes).map(|i| ("png", i))
    } else if bytes.starts_with(b"GIF8") {
        decode_gif(bytes).map(|i| ("gif", i))
    } else {
        Err(format!("not an image ({} bytes)", bytes.len()))
    };
    match decoded {
        Ok((kind, (w, h, mut rgba))) => {
            for px in rgba.chunks_mut(4) {
                if px[3] == 0 {
                    px[..3].fill(0);
                }
            }
            format!("{kind} {w}x{h}, pixels fnv {:016x}", super::web::fnv(&rgba))
        }
        Err(e) => e,
    }
}

type Image = (u32, u32, Vec<u8>);

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("bad png: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("bad png: {e}"))?;
    let (w, h) = (info.width, info.height);
    let px = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3]
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2]
            .chunks(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("bad png: indexed after expansion".into()),
    };
    Ok((w, h, rgba))
}

/// The first frame of a GIF, on a transparent canvas the size of the logical screen.
fn decode_gif(b: &[u8]) -> Result<Image, String> {
    let bad = || "bad gif".to_string();
    let u16le = |i: usize| -> Result<usize, String> {
        Ok(*b.get(i).ok_or_else(bad)? as usize | (*b.get(i + 1).ok_or_else(bad)? as usize) << 8)
    };
    let (sw, sh) = (u16le(6)?, u16le(8)?);
    let flags = *b.get(10).ok_or_else(bad)?;
    let mut pos = 13;
    let mut global = Vec::new();
    if flags & 0x80 != 0 {
        let n = 3 << ((flags & 7) + 1);
        global = b.get(pos..pos + n).ok_or_else(bad)?.to_vec();
        pos += n;
    }
    let mut transparent: Option<u8> = None;
    loop {
        match *b.get(pos).ok_or_else(bad)? {
            0x21 => {
                let label = *b.get(pos + 1).ok_or_else(bad)?;
                pos += 2;
                if label == 0xf9 && b.get(pos) == Some(&4) && b[pos + 1] & 1 != 0 {
                    transparent = Some(*b.get(pos + 4).ok_or_else(bad)?);
                }
                while *b.get(pos).ok_or_else(bad)? != 0 {
                    pos += b[pos] as usize + 1;
                }
                pos += 1;
            }
            0x2c => {
                let (x, y, w, h) = (
                    u16le(pos + 1)?,
                    u16le(pos + 3)?,
                    u16le(pos + 5)?,
                    u16le(pos + 7)?,
                );
                let f = *b.get(pos + 9).ok_or_else(bad)?;
                pos += 10;
                let mut table = &global[..];
                let local;
                if f & 0x80 != 0 {
                    let n = 3 << ((f & 7) + 1);
                    local = b.get(pos..pos + n).ok_or_else(bad)?.to_vec();
                    table = &local[..];
                    pos += n;
                }
                let min_code = *b.get(pos).ok_or_else(bad)?;
                pos += 1;
                let mut data = Vec::new();
                while *b.get(pos).ok_or_else(bad)? != 0 {
                    let n = b[pos] as usize;
                    data.extend_from_slice(b.get(pos + 1..pos + 1 + n).ok_or_else(bad)?);
                    pos += n + 1;
                }
                let indices = lzw_decode(&data, min_code, w * h).ok_or_else(bad)?;
                let interlaced = f & 0x40 != 0;
                let rows: Vec<usize> = if interlaced {
                    [(0, 8), (4, 8), (2, 4), (1, 2)]
                        .iter()
                        .flat_map(|&(s, step)| (s..h).step_by(step))
                        .collect()
                } else {
                    (0..h).collect()
                };
                let mut rgba = vec![0u8; sw * sh * 4];
                for (row_i, &row) in rows.iter().enumerate() {
                    for col in 0..w {
                        let Some(&ix) = indices.get(row_i * w + col) else {
                            continue;
                        };
                        let (px, py) = (x + col, y + row);
                        if px >= sw || py >= sh || Some(ix) == transparent {
                            continue;
                        }
                        let c = table
                            .get(ix as usize * 3..ix as usize * 3 + 3)
                            .ok_or_else(bad)?;
                        let o = (py * sw + px) * 4;
                        rgba[o..o + 3].copy_from_slice(c);
                        rgba[o + 3] = 255;
                    }
                }
                return Ok((sw as u32, sh as u32, rgba));
            }
            _ => return Err(bad()),
        }
    }
}

fn lzw_decode(data: &[u8], min_code: u8, max: usize) -> Option<Vec<u8>> {
    if !(1..=11).contains(&min_code) {
        return None;
    }
    let clear = 1usize << min_code;
    let end = clear + 1;
    let mut dict: Vec<Vec<u8>> = Vec::new();
    let reset = |dict: &mut Vec<Vec<u8>>| {
        dict.clear();
        dict.extend((0..clear).map(|i| vec![i as u8]));
        dict.push(Vec::new());
        dict.push(Vec::new());
    };
    reset(&mut dict);
    let mut size = min_code as u32 + 1;
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut prev: Option<usize> = None;
    let mut out = Vec::new();
    for &byte in data {
        acc |= (byte as u32) << bits;
        bits += 8;
        while bits >= size {
            let code = (acc & ((1 << size) - 1)) as usize;
            acc >>= size;
            bits -= size;
            if code == clear {
                reset(&mut dict);
                size = min_code as u32 + 1;
                prev = None;
                continue;
            }
            if code == end {
                return Some(out);
            }
            let entry = if code < dict.len() {
                dict[code].clone()
            } else {
                let p = &dict[prev?];
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            if let Some(p) = prev
                && dict.len() < 4096
            {
                let mut n = dict[p].clone();
                n.push(entry[0]);
                dict.push(n);
                if dict.len() == (1 << size) && size < 12 {
                    size += 1;
                }
            }
            out.extend_from_slice(&entry);
            if out.len() >= max {
                return Some(out);
            }
            prev = Some(code);
        }
    }
    Some(out)
}
