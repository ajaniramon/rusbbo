//! Java: avatara4j 1.x (`Avatar`, `Badge`, their readers), which `ImagerController` draws avatars
//! and badges with. Its resources (the `figuredata/` and `badges/` folders of the jar) are read
//! from `--web-imaging`.
//!
//! Pixels match Java's: Java2D's `SrcOver` blending is reproduced with its 8-bit multiply and
//! divide tables, and the quirks are kept (sets keyed by id only, the swapped canvas sizes given
//! to `AvatarAsset`, figure parts in `HashMap` order, the badge position 6 that uses the width).
//! The encoders are not ImageIO's, so the files differ byte for byte.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

// ---------------------------------------------------------------------------------------------
// Images

/// A `BufferedImage.TYPE_INT_ARGB`: non-premultiplied `0xAARRGGBB`.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

/// Java2D's `mul8table[a][b]`.
fn mul8(a: u32, b: u32) -> u32 {
    if a == 0 || b == 0 {
        return 0;
    }
    ((a as u64 * 0x010101 * b as u64 + (1 << 23)) >> 24) as u32
}

/// Java2D's `div8table[a][v]` (`v / a`, scaled to 255).
fn div8(v: u32, a: u32) -> u32 {
    if v >= a {
        return 255;
    }
    let inc = ((0xffu64 << 24) + a as u64 / 2) / a as u64;
    (((1u64 << 23) + v as u64 * inc) >> 24) as u32
}

/// One pixel of `IntArgb -> IntArgb` `SrcOver` (`DEFINE_SRCOVER_MASKBLIT`, extra alpha 1.0).
fn src_over(dst: u32, src: u32) -> u32 {
    let sa = src >> 24;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }
    let ch = |p: u32, s: u32| (p >> s) & 0xff;
    let da = mul8(255 - sa, dst >> 24);
    let ra = sa + da;
    let mut out = ra << 24;
    for s in [16, 8, 0] {
        let mut c = mul8(sa, ch(src, s)) + mul8(da, ch(dst, s));
        if ra > 0 && ra < 255 {
            c = div8(c, ra);
        }
        out |= (c & 0xff) << s;
    }
    out
}

impl Canvas {
    /// `ColorExtensions.createTransparentImage`.
    pub fn new(w: i32, h: i32) -> Self {
        Canvas {
            w,
            h,
            px: vec![0; (w.max(0) * h.max(0)) as usize],
        }
    }

    /// `ImageIO.read` of a PNG.
    pub fn decode_png(bytes: &[u8]) -> Option<Self> {
        let mut decoder = png::Decoder::new(bytes);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        let n = (info.width * info.height) as usize;
        let argb = |r: u8, g: u8, b: u8, a: u8| {
            (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
        };
        let px: Vec<u32> = match info.color_type {
            png::ColorType::Rgba => buf[..n * 4]
                .chunks(4)
                .map(|c| argb(c[0], c[1], c[2], c[3]))
                .collect(),
            png::ColorType::Rgb => buf[..n * 3]
                .chunks(3)
                .map(|c| argb(c[0], c[1], c[2], 255))
                .collect(),
            png::ColorType::GrayscaleAlpha => buf[..n * 2]
                .chunks(2)
                .map(|c| argb(c[0], c[0], c[0], c[1]))
                .collect(),
            png::ColorType::Grayscale => buf[..n].iter().map(|&g| argb(g, g, g, 255)).collect(),
            png::ColorType::Indexed => return None,
        };
        Some(Canvas {
            w: info.width as i32,
            h: info.height as i32,
            px,
        })
    }

    /// `Graphics2D.drawImage(src, x, y, null)` (default `SrcOver`, clipped to the canvas).
    pub fn draw(&mut self, src: &Canvas, x: i32, y: i32) {
        for sy in 0..src.h {
            let dy = y + sy;
            if dy < 0 || dy >= self.h {
                continue;
            }
            for sx in 0..src.w {
                let dx = x + sx;
                if dx < 0 || dx >= self.w {
                    continue;
                }
                let d = (dy * self.w + dx) as usize;
                self.px[d] = src_over(self.px[d], src.px[(sy * src.w + sx) as usize]);
            }
        }
    }

    /// `AffineTransformOp(scale(-1, 1), NEAREST_NEIGHBOR).filter(img, null)`.
    pub fn flipped(&self) -> Canvas {
        let mut out = Canvas::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                out.px[(y * self.w + x) as usize] =
                    self.px[(y * self.w + (self.w - 1 - x)) as usize];
            }
        }
        out
    }

    /// `Avatar.resizeAndToBytes`: drawn twice as big, nearest neighbour, on a new transparent
    /// image (so translucent pixels go through `SrcOver` once more).
    pub fn doubled(&self) -> Canvas {
        let mut out = Canvas::new(self.w * 2, self.h * 2);
        for y in 0..out.h {
            for x in 0..out.w {
                let i = (y * out.w + x) as usize;
                out.px[i] = src_over(0, self.px[((y / 2) * self.w + x / 2) as usize]);
            }
        }
        out
    }

    /// `ColorExtensions.tintImage(image, hex, alpha)`: every pixel that is not fully transparent
    /// becomes `channel * tint / 255` with the given alpha.
    pub fn tint(&mut self, (r, g, b): (u32, u32, u32), alpha: u32) {
        for p in self.px.iter_mut() {
            if *p >> 24 == 0 {
                continue;
            }
            let c = |s: u32, t: u32| ((*p >> s) & 0xff) * t / 255;
            *p = alpha << 24 | c(16, r) << 16 | c(8, g) << 8 | c(0, b);
        }
    }

    pub fn png(&self) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.w as u32, self.h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = enc.write_header() {
                let _ = writer.write_image_data(&rgba);
            }
        }
        out
    }

    /// ImageIO's GIF writer on a translucent image: every pixel that is not opaque becomes the
    /// transparent index 0 (`PaletteBuilder`); opaque colours keep their value while they fit in
    /// the palette.
    pub fn gif(&self) -> Vec<u8> {
        let transparent = self.px.iter().any(|p| p >> 24 != 255);
        let mut palette: Vec<u32> = Vec::new();
        if transparent {
            palette.push(0);
        }
        let mut index: HashMap<u32, u8> = HashMap::new();
        let mut colours: Vec<u32> = self
            .px
            .iter()
            .filter(|p| *p >> 24 == 255)
            .map(|p| p & 0xffffff)
            .collect();
        colours.sort_unstable();
        colours.dedup();
        let room = 256 - palette.len();
        let reduced = reduce_colours(&colours, room);
        for (c, r) in colours.iter().zip(reduced.iter()) {
            let i = match palette
                .iter()
                .skip(transparent as usize)
                .position(|p| p == r)
            {
                Some(i) => i + transparent as usize,
                None => {
                    palette.push(*r);
                    palette.len() - 1
                }
            };
            index.insert(*c, i as u8);
        }
        let pixels: Vec<u8> = self
            .px
            .iter()
            .map(|p| {
                if *p >> 24 != 255 {
                    0
                } else {
                    index[&(p & 0xffffff)]
                }
            })
            .collect();
        let mut bits = 1;
        while (1usize << bits) < palette.len().max(2) {
            bits += 1;
        }
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&(self.w as u16).to_le_bytes());
        out.extend_from_slice(&(self.h as u16).to_le_bytes());
        out.push(0x80 | 0x70 | (bits - 1) as u8);
        out.extend_from_slice(&[0, 0]);
        for i in 0..(1usize << bits) {
            let c = palette.get(i).copied().unwrap_or(0);
            out.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
        if transparent {
            out.extend_from_slice(&[0x21, 0xf9, 4, 0x09, 0, 0, 0, 0]);
        }
        out.push(0x2c);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(self.w as u16).to_le_bytes());
        out.extend_from_slice(&(self.h as u16).to_le_bytes());
        out.push(0);
        let min_code = bits.max(2) as u8;
        out.push(min_code);
        let data = lzw_encode(&pixels, min_code);
        for chunk in data.chunks(255) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        out.push(0);
        out.push(0x3b);
        out
    }
}

/// When there are more colours than palette entries, the closest ones are merged (ImageIO
/// prunes an octree; badges have far fewer colours than that).
fn reduce_colours(colours: &[u32], room: usize) -> Vec<u32> {
    if colours.len() <= room {
        return colours.to_vec();
    }
    // Merge by dropping low bits until the colours fit.
    for shift in 1..8 {
        let mask = (0xffu32 << shift) & 0xff;
        let m = |c: u32| c & (mask << 16 | mask << 8 | mask);
        let mut set: Vec<u32> = colours.iter().map(|&c| m(c)).collect();
        set.sort_unstable();
        set.dedup();
        if set.len() <= room {
            return colours.iter().map(|&c| m(c)).collect();
        }
    }
    colours.iter().map(|_| 0).collect()
}

fn lzw_encode(pixels: &[u8], min_code: u8) -> Vec<u8> {
    let clear = 1u32 << min_code;
    let end = clear + 1;
    let mut out = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    let mut size = min_code as u32 + 1;
    let mut emit = |code: u32, size: u32, out: &mut Vec<u8>| {
        acc |= code << nbits;
        nbits += size;
        while nbits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            nbits -= 8;
        }
    };
    let mut dict: HashMap<(u32, u8), u32> = HashMap::new();
    let mut next = end + 1;
    emit(clear, size, &mut out);
    let mut cur: Option<u32> = None;
    for &p in pixels {
        let Some(c) = cur else {
            cur = Some(p as u32);
            continue;
        };
        if let Some(&code) = dict.get(&(c, p)) {
            cur = Some(code);
            continue;
        }
        emit(c, size, &mut out);
        if next < 4096 {
            dict.insert((c, p), next);
            next += 1;
            if next > (1 << size) && size < 12 {
                size += 1;
            }
        } else {
            emit(clear, size, &mut out);
            dict.clear();
            next = end + 1;
            size = min_code as u32 + 1;
        }
        cur = Some(p as u32);
    }
    if let Some(c) = cur {
        emit(c, size, &mut out);
        // The decoder grows its code size one code earlier than the encoder adds the entry.
        if next < 4096 && next + 1 > (1 << size) && size < 12 {
            size += 1;
        }
    }
    emit(end, size, &mut out);
    if nbits > 0 {
        out.push(acc as u8);
    }
    out
}

/// `ColorExtensions.hexToColor` (`None` where Java throws).
fn hex_to_rgb(hex: &str) -> Option<(u32, u32, u32)> {
    if hex.eq_ignore_ascii_case("transparent") {
        return Some((0, 0, 0));
    }
    let hex = hex.replace('#', "");
    let part = |a: usize, b: usize| -> Option<u32> {
        let s = hex.get(a..b)?;
        let v = i32::from_str_radix(s, 16).ok()?;
        (0..=255).contains(&v).then_some(v as u32)
    };
    Some((part(0, 2)?, part(2, 4)?, part(4, 6)?))
}

/// `String.split` with a literal separator (trailing empty strings removed).
fn java_split(s: &str, sep: char) -> Vec<&str> {
    let mut v: Vec<&str> = s.split(sep).collect();
    while v.len() > 1 && v.last() == Some(&"") {
        v.pop();
    }
    if v.len() == 1 && v[0].is_empty() && !s.is_empty() {
        v.clear();
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Figure data

#[derive(Clone, Debug)]
struct Part {
    id: String,
    kind: String,
    colorable: bool,
    order: i32,
}

/// `FigurePart.getOrder`.
fn part_order(kind: &str) -> i32 {
    match kind {
        "sh" => 5,
        "lg" => 6,
        "ch" => 7,
        "wa" => 8,
        "ca" => 9,
        "fa" => 27,
        "ea" => 28,
        "ha" | "he" => 29,
        "cc" | "bd" => 1,
        "cp" => 6,
        "hd" => 22,
        "fc" | "lc" => 23,
        "hr" | "ey" | "rc" => 24,
        "lh" => 5,
        "ls" => 7,
        "rh" => 10,
        "rs" => 11,
        "li" => 0,
        "hrb" => 26,
        "ri" => 9,
        "fx" => 100,
        _ => -1,
    }
}

impl Part {
    fn new(id: &str, kind: &str, colorable: bool) -> Self {
        Part {
            id: id.to_string(),
            kind: kind.to_string(),
            colorable,
            order: part_order(kind),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Set {
    parts: Vec<Part>,
    hidden_layers: Vec<String>,
}

#[derive(Clone, Debug)]
struct LegacyFigure {
    sprite: i32,
    parts: Vec<String>,
    colours: Vec<String>,
}

/// A decoded image file: `Err` when there is no such file, `Ok(None)` when it is not an image.
type Loaded = Result<Option<Arc<Canvas>>, ()>;

/// avatara4j's readers (`FiguredataReader`, `LegacyFiguredataReader`, `ManifestReader`) and its
/// image files.
pub struct Imager {
    root: PathBuf,
    /// By set id: Java keys its map by id only, so a later set with the same id wins.
    sets: HashMap<String, Arc<Set>>,
    /// Set type -> palette id.
    set_types: HashMap<String, i32>,
    /// Palette id -> (colour id, hex), in document order.
    palettes: HashMap<i32, Vec<(String, String)>>,
    /// Palette ids in Java `HashMap<Integer, _>` order.
    palette_order: Vec<i32>,
    legacy: Vec<LegacyFigure>,
    /// Asset name -> its `offset` param.
    offsets: HashMap<String, String>,
    images: Mutex<HashMap<PathBuf, Loaded>>,
}

fn attr<'a>(n: &roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}

fn text(n: &roxmltree::Node) -> String {
    n.descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect()
}

impl Imager {
    pub fn load(root: &Path) -> Self {
        let mut im = Imager {
            root: root.to_path_buf(),
            sets: HashMap::new(),
            set_types: HashMap::new(),
            palettes: HashMap::new(),
            palette_order: Vec::new(),
            legacy: Vec::new(),
            offsets: HashMap::new(),
            images: Mutex::new(HashMap::new()),
        };
        let read = |p: PathBuf| match std::fs::read_to_string(&p) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!("imager: {}: {e}", p.display());
                None
            }
        };
        if let Some(t) = read(root.join("figuredata/figuredata.xml")) {
            match roxmltree::Document::parse(&t) {
                Ok(doc) => im.load_figuredata(&doc),
                Err(e) => tracing::warn!("imager: figuredata.xml: {e}"),
            }
        }
        if let Some(t) = read(root.join("figuredata/figuredata_oldschool.xml")) {
            match roxmltree::Document::parse(&t) {
                Ok(doc) => im.load_legacy(&doc),
                Err(e) => tracing::warn!("imager: figuredata_oldschool.xml: {e}"),
            }
        }
        let mut manifests: Vec<PathBuf> = std::fs::read_dir(root.join("figuredata/xml"))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        manifests.sort();
        for path in manifests {
            let Some(t) = read(path.clone()) else {
                continue;
            };
            match roxmltree::Document::parse(&t) {
                Ok(doc) => im.load_manifest(&doc),
                Err(e) => tracing::warn!("imager: {}: {e}", path.display()),
            }
        }
        tracing::info!("Loaded {} figure offsets!", im.offsets.len());
        im
    }

    fn load_figuredata(&mut self, doc: &roxmltree::Document) {
        for set in doc.descendants().filter(|n| n.has_tag_name("set")) {
            let (Some(id), Some(_), Some(_), Some(_), Some(_)) = (
                attr(&set, "id"),
                attr(&set, "gender"),
                attr(&set, "club"),
                attr(&set, "colorable"),
                attr(&set, "selectable"),
            ) else {
                // Java stops loading at the first set without one of them.
                tracing::warn!("imager: figuredata set without its attributes");
                return;
            };
            let mut s = Set::default();
            for p in set.children().filter(|n| n.is_element()) {
                if p.has_tag_name("hiddenlayers") {
                    continue;
                }
                if let (Some(pid), Some(kind), Some(col), Some(_)) = (
                    attr(&p, "id"),
                    attr(&p, "type"),
                    attr(&p, "colorable"),
                    attr(&p, "index"),
                ) {
                    s.parts.push(Part::new(pid, kind, col == "1"));
                }
            }
            for h in set.children().filter(|n| n.has_tag_name("hiddenlayers")) {
                for l in h.children().filter(|n| n.is_element()) {
                    if let Some(t) = attr(&l, "parttype") {
                        s.hidden_layers.push(t.to_string());
                    }
                }
            }
            self.sets.insert(id.to_string(), Arc::new(s));
        }
        for st in doc.descendants().filter(|n| n.has_tag_name("settype")) {
            if let (Some(kind), Some(pid)) = (
                attr(&st, "type"),
                attr(&st, "paletteid").and_then(|v| v.parse::<i32>().ok()),
            ) {
                self.set_types.insert(kind.to_string(), pid);
            }
        }
        let mut ids = Vec::new();
        for pal in doc.descendants().filter(|n| n.has_tag_name("palette")) {
            let Some(pid) = attr(&pal, "id").and_then(|v| v.parse::<i32>().ok()) else {
                continue;
            };
            let colours = pal
                .children()
                .filter(|c| c.is_element())
                .filter(|c| {
                    ["id", "index", "club", "selectable"]
                        .iter()
                        .all(|a| c.attribute(*a).is_some())
                })
                .map(|c| (attr(&c, "id").unwrap_or("").to_string(), text(&c)))
                .collect();
            if !self.palettes.contains_key(&pid) {
                ids.push(pid);
            }
            self.palettes.insert(pid, colours);
        }
        self.palette_order = super::data::java_int_map_order(ids);
    }

    fn load_legacy(&mut self, doc: &roxmltree::Document) {
        for genders in doc.descendants().filter(|n| n.has_tag_name("genders")) {
            for gender in genders.children().filter(|n| n.is_element()) {
                for fig in gender.children().filter(|n| n.is_element()) {
                    let sprite = fig
                        .descendants()
                        .find(|n| n.has_tag_name("sprite"))
                        .and_then(|n| text(&n).parse::<i32>().ok());
                    let Some(sprite) = sprite else {
                        // Java's load stops at the first figure it cannot parse.
                        return;
                    };
                    let parts = fig
                        .descendants()
                        .find(|n| n.has_tag_name("parts"))
                        .map(|p| {
                            p.children()
                                .filter(|n| n.is_element())
                                .map(|n| n.tag_name().name().to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    let colours = fig
                        .descendants()
                        .filter(|n| n.has_tag_name("colour"))
                        .map(|n| text(&n))
                        .collect();
                    self.legacy.push(LegacyFigure {
                        sprite,
                        parts,
                        colours,
                    });
                }
            }
        }
    }

    fn load_manifest(&mut self, doc: &roxmltree::Document) {
        for asset in doc.descendants().filter(|n| n.has_tag_name("asset")) {
            let Some(name) = attr(&asset, "name") else {
                continue;
            };
            if java_split(name, '_').len() < 3 {
                continue;
            }
            let offset = asset
                .children()
                .filter(|c| c.has_tag_name("param"))
                .find(|c| attr(c, "key") == Some("offset") && attr(c, "value").is_some())
                .and_then(|c| attr(&c, "value"));
            if let Some(offset) = offset {
                self.offsets
                    .entry(name.to_string())
                    .or_insert_with(|| offset.to_string());
            }
        }
    }

    /// `ImageIO.read(FileUtil.getFile(dir, name))`: `Err` where Java throws (no such file),
    /// `Ok(None)` where it returns `null` (not an image).
    fn image(&self, rel: &str) -> Result<Option<Arc<Canvas>>, ()> {
        let path = self.root.join(rel);
        if let Some(i) = self.images.lock().get(&path) {
            return i.clone();
        }
        let loaded = std::fs::read(&path)
            .map(|b| Canvas::decode_png(&b).map(Arc::new))
            .map_err(|_| ());
        self.images.lock().insert(path, loaded.clone());
        loaded
    }

    fn image_exists(&self, rel: &str) -> bool {
        self.root.join(rel).is_file()
    }
}

// ---------------------------------------------------------------------------------------------
// FigureConverter (numeric figures)

impl Imager {
    /// `FigureConverter.convertOldToNew` (`None` where Java throws).
    fn convert_old_to_new(&self, old: &str) -> Option<String> {
        if old.len() < 25 {
            return None;
        }
        let mut parts = [0i32; 10];
        let mut start = 0;
        for (i, p) in parts.iter_mut().enumerate() {
            let len = if i % 2 == 0 { 3 } else { 2 };
            *p = old.get(start..start + len)?.parse().ok()?;
            start += len;
        }
        let hr = self.convert_colour("hr", parts[0], parts[1])?;
        let hr_colour: i32 = hr.as_deref()?.parse().ok()?;
        let s = |c: Option<String>| c.unwrap_or_else(|| "null".into());
        let mut out = format!("hr-{}-{}", parts[0], s(hr));
        out += &format!(
            ".hd-{}-{}",
            parts[2],
            s(self.convert_colour("hd", parts[2], parts[3])?)
        );
        out += &format!(
            ".ch-{}-{}",
            parts[8],
            s(self.convert_colour("ch", parts[8], parts[9])?)
        );
        out += &format!(
            ".lg-{}-{}",
            parts[4],
            s(self.convert_colour("lg", parts[4], parts[5])?)
        );
        out += &format!(
            ".sh-{}-{}",
            if parts[6] == 730 { 3206 } else { parts[6] },
            s(self.convert_colour("sh", parts[6], parts[7])?)
        );
        out += &hats(parts[0], hr_colour);
        Some(out)
    }

    /// `convertOldColorToNew`: `Some(None)` is Java's `null`, `None` an exception.
    fn convert_colour(&self, part: &str, sprite: i32, index: i32) -> Option<Option<String>> {
        let found = self.legacy.iter().find(|f| {
            f.sprite == sprite
                && f.parts.iter().any(|p| p == part)
                && f.colours.len() as i64 >= index as i64
        });
        let Some(f) = found else {
            return Some(None);
        };
        let old = f.colours.get(usize::try_from(index - 1).ok()?)?;
        for pid in &self.palette_order {
            if let Some(c) = self.palettes[pid]
                .iter()
                .find(|(_, hex)| hex.eq_ignore_ascii_case(old))
            {
                return Some(Some(c.0.clone()));
            }
        }
        Some(None)
    }
}

/// `FigureConverter.takeCareOfHats`.
fn hats(sprite: i32, c: i32) -> String {
    match sprite {
        120 => ".ha-1001-0".into(),
        140 | 525 => format!(".ha-1002-{c}"),
        150 | 535 => format!(".ha-1003-{c}"),
        160 | 565 => format!(".ha-1004-{c}"),
        570 => format!(".ha-1005-{c}"),
        175 | 585 => ".ha-1006-0".into(),
        176 | 580 => ".ha-1007-0.fa-1202-70".into(),
        177 | 590 => ".ha-1008-0".into(),
        178 | 595 => ".ha-1009-1321".into(),
        130 => format!(".ha-1010-{c}"),
        801 => format!(".hr-829-{c}.fa-1201-62.ha-1011-{c}"),
        800 | 810 => format!(".ha-1012-{c}"),
        802 | 811 => format!(".ha-1013-{c}"),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Avatar

/// The request `ImagerController.avatarimage` builds an `Avatar` from.
#[derive(Debug, Clone)]
pub struct AvatarRequest {
    pub figure: String,
    pub size: String,
    pub body_direction: i32,
    pub head_direction: i32,
    pub action: String,
    pub gesture: String,
    pub head_only: bool,
    pub frame: i32,
    pub carry_drink: i32,
}

struct Asset {
    name: String,
    file: String,
    image_x: i32,
    image_y: i32,
    part: Part,
    set: Arc<Set>,
    parts: Option<Vec<String>>,
    order: i32,
    drink_canvas: bool,
}

struct Avatar<'a> {
    im: &'a Imager,
    figure: String,
    size: String,
    small: bool,
    body_direction: i32,
    head_direction: i32,
    action: String,
    gesture: String,
    frame: i32,
    carry_drink: i32,
    entire: bool,
    width: i32,
    height: i32,
}

/// `isHead`.
fn is_head(kind: &str) -> bool {
    ["hr", "hd", "he", "ha", "ea", "fa", "ey", "fc"]
        .iter()
        .any(|k| kind.contains(k))
}

/// Directions 4, 5 and 6 are drawn as 2, 1 and 0, flipped.
fn mirrored(d: i32) -> i32 {
    match d {
        4 => 2,
        6 => 0,
        5 => 1,
        d => d,
    }
}

/// The outcome of `Avatar.run()`: `Err` where Java throws.
pub enum Rendered {
    Png(Vec<u8>),
    /// `run()` returned `null` (a figure part without a set id).
    Null,
}

impl Imager {
    /// `new Avatar(...).run()`.
    pub fn avatar(&self, req: &AvatarRequest) -> Result<Rendered, ()> {
        let mut figure = req.figure.clone();
        if !figure.is_empty() && figure.bytes().all(|b| b.is_ascii_digit()) {
            figure = self.convert_old_to_new(&figure).ok_or(())?;
        }
        let small = !req.size.eq_ignore_ascii_case("b") && !req.size.eq_ignore_ascii_case("l");
        let (mut width, mut height) = (64, 110);
        if small {
            width /= 2;
            height /= 2;
        }
        let mut a = Avatar {
            im: self,
            figure,
            size: req.size.to_lowercase(),
            small,
            body_direction: req.body_direction,
            head_direction: req.head_direction,
            action: req.action.clone(),
            gesture: req.gesture.clone(),
            frame: req.frame - 1,
            carry_drink: req.carry_drink,
            entire: !req.head_only,
            width,
            height,
        };
        if a.action == "lay" {
            std::mem::swap(&mut a.width, &mut a.height);
            a.gesture = "lay".into();
            a.head_direction = a.body_direction;
            if a.body_direction != 2 && a.body_direction != 4 {
                a.body_direction = 2;
            }
            if a.head_direction != 2 && a.head_direction != 4 {
                a.head_direction = 2;
            }
            a.carry_drink = 0;
        }
        let Some(queue) = a.build_draw_queue()? else {
            return Ok(Rendered::Null);
        };
        Ok(Rendered::Png(a.draw_image(&queue).png()))
    }
}

impl Avatar<'_> {
    fn build_draw_queue(&self) -> Result<Option<Vec<Asset>>, ()> {
        // HashMap<String, String> figureData: part type -> the part, iterated in hash order.
        let mut figure_data: Vec<(String, String)> = Vec::new();
        for data in java_split(&self.figure, '.') {
            let parts = java_split(data, '-');
            let key = parts.first().copied().unwrap_or("").to_string();
            let value = parts.join("-");
            match figure_data.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = value,
                None => figure_data.push((key, value)),
            }
        }
        let mut queue: Vec<Asset> = Vec::new();
        for (_, data) in super::data::java_hashmap_order(figure_data) {
            let parts: Vec<String> = java_split(&data, '-')
                .into_iter()
                .map(str::to_string)
                .collect();
            if parts.len() < 2 {
                return Ok(None);
            }
            let Some(set) = self.im.sets.get(&parts[1]) else {
                continue;
            };
            for part in &set.parts {
                if let Some(a) = self.load_figure_asset(&parts, part, set)? {
                    queue.push(a);
                }
            }
        }
        let head_order = queue
            .iter()
            .filter(|a| is_head(&a.part.kind))
            .map(|a| a.order)
            .max()
            .unwrap_or(1);
        if self.carry_drink > 0
            && let Some(mut carry) = self.load_carry_item_asset(self.carry_drink)?
        {
            if matches!(self.body_direction, 1 | 5 | 6 | 0) {
                carry.drink_canvas = false;
                carry.order = 0;
            } else {
                carry.order = head_order + 1;
                carry.drink_canvas = true;
            }
            if self.action == "drk" {
                for a in queue.iter_mut() {
                    if a.name.contains("_drk_") {
                        a.order = 100;
                        a.drink_canvas = true;
                    }
                }
            }
            queue.push(carry);
        }
        queue.sort_by_key(|a| a.order);
        Ok(Some(queue))
    }

    fn load_carry_item_asset(&self, carry: i32) -> Result<Option<Asset>, ()> {
        let direction = mirrored(self.body_direction);
        let part = Part::new("0", "ri", false);
        let set = Arc::new(Set::default());
        let prefix = if self.small { "sh" } else { "h" };
        for name in [
            format!("{prefix}_{}_ri_{carry}_{direction}_0", self.action),
            format!("{prefix}_crr_ri_{carry}_{direction}_0"),
            format!("{prefix}_std_ri_{carry}_0_0"),
        ] {
            if let Some(a) = self.locate_asset(&name, None, &part, &set)? {
                return Ok(Some(a));
            }
        }
        Ok(None)
    }

    fn load_figure_asset(
        &self,
        parts: &[String],
        part: &Part,
        set: &Arc<Set>,
    ) -> Result<Option<Asset>, ()> {
        let kind = part.kind.as_str();
        let (mut direction, mut gesture) = if is_head(kind) {
            (mirrored(self.head_direction), self.gesture.as_str())
        } else {
            (mirrored(self.body_direction), self.action.as_str())
        };
        if direction == 1 && kind == "ls" {
            return Ok(None);
        }
        if self.action == "lay" && self.body_direction == 4 {
            direction = 2;
        }
        if self.carry_drink > 0
            && self.action != "lay"
            && self.action != "drk"
            && (kind == "ls" || kind == "lh")
        {
            gesture = "std";
        }
        let prefix = if self.small { "sh" } else { "h" };
        let (id, frame) = (&part.id, self.frame);
        let mut names = Vec::new();
        if self.carry_drink > 0
            && (kind == "rs" || kind == "rh")
            && self.action != "drk"
            && self.action != "crr"
        {
            names.push(format!("{prefix}_crr_{kind}_{id}_{direction}_0"));
        }
        names.push(format!(
            "{prefix}_{gesture}_{kind}_{id}_{direction}_{frame}"
        ));
        names.push(format!("{prefix}_std_{kind}_{id}_{direction}_{frame}"));
        names.push(format!("{prefix}_std_{kind}_{id}_{direction}_0"));
        if self.small {
            names.push(format!("{prefix}_std_{kind}_1_{direction}_{frame}"));
        }
        for name in names {
            if let Some(a) = self.locate_asset(&name, Some(parts), part, set)? {
                return Ok(Some(a));
            }
        }
        Ok(None)
    }

    fn locate_asset(
        &self,
        name: &str,
        parts: Option<&[String]>,
        part: &Part,
        set: &Arc<Set>,
    ) -> Result<Option<Asset>, ()> {
        let Some(offsets) = self.im.offsets.get(name) else {
            return Ok(None);
        };
        if name.contains(['/', '\\']) || name.contains("..") {
            return Ok(None);
        }
        let file = format!("figuredata/images/{name}.png");
        if !self.im.image_exists(&file) {
            return Ok(None);
        }
        let o = java_split(offsets, ',');
        let x: i32 = o.first().ok_or(())?.parse().map_err(|_| ())?;
        let y: i32 = o.get(1).ok_or(())?.parse().map_err(|_| ())?;
        // AvatarAsset's constructor takes (canvasW, canvasH) but is given (height, width).
        let (cw, ch) = (self.height, self.width);
        let (image_x, image_y) = if self.action == "lay" {
            (
                x + ch / 2 - if self.small { -11 } else { -10 },
                y + cw / 2 + if self.small { -5 } else { -20 },
            )
        } else {
            (
                x + cw / 2 + if self.small { 5 } else { 10 },
                y + ch / 2 - if self.small { 11 } else { 20 },
            )
        };
        Ok(Some(Asset {
            name: name.to_string(),
            file,
            image_x,
            image_y,
            part: part.clone(),
            set: set.clone(),
            parts: parts.map(|p| p.to_vec()),
            order: part.order,
            drink_canvas: false,
        }))
    }

    fn draw_image(&self, queue: &[Asset]) -> Canvas {
        let mut body = Canvas::new(self.width, self.height);
        let mut face = Canvas::new(self.width, self.height);
        let mut drink = Canvas::new(self.width, self.height);
        for asset in queue {
            if !self.entire && !is_head(&asset.part.kind) {
                continue;
            }
            self.draw_asset(queue, &mut body, &mut face, &mut drink, asset);
        }
        if (4..=6).contains(&self.head_direction) {
            face = face.flipped();
        }
        if (4..=6).contains(&self.body_direction) {
            body = body.flipped();
            drink = drink.flipped();
        }
        body.draw(&face, 0, 0);
        body.draw(&drink, 0, 0);
        let canvas = if self.entire { body } else { face };
        if self.size == "l" {
            canvas.doubled()
        } else {
            canvas
        }
    }

    /// `drawAsset`: any exception skips the asset.
    fn draw_asset(
        &self,
        queue: &[Asset],
        body: &mut Canvas,
        face: &mut Canvas,
        drink: &mut Canvas,
        asset: &Asset,
    ) {
        let Ok(Some(image)) = self.im.image(&asset.file) else {
            return;
        };
        let kind = asset.part.kind.as_str();
        if queue
            .iter()
            .any(|x| x.set.hidden_layers.iter().any(|l| l == kind))
        {
            return;
        }
        let mut image = (*image).clone();
        if kind != "ey" {
            if let Some(parts) = &asset.parts
                && asset.part.colorable
                && parts.len() > 2
                && !parts[2].is_empty()
                && parts[2].parse::<i32>().is_ok()
                && let Some(pid) = self.im.set_types.get(&parts[0])
            {
                let Some(palette) = self.im.palettes.get(pid) else {
                    return;
                };
                if let Some((_, hex)) = palette.iter().find(|(id, _)| *id == parts[2]) {
                    let Some(rgb) = hex_to_rgb(hex) else {
                        return;
                    };
                    image.tint(rgb, 255);
                }
            }
        } else {
            image.tint((255, 255, 255), 255);
        }
        let target: &mut Canvas = if is_head(kind) {
            face
        } else if asset.drink_canvas {
            drink
        } else {
            body
        };
        // The point is taken from the face or body canvas (all three are the same size).
        let x = target.w - asset.image_x + 1;
        let y = target.h - asset.image_y + 2;
        target.draw(&image, x, y);
    }
}

// ---------------------------------------------------------------------------------------------
// Badges

const BADGE_COLOURS: [&str; 18] = [
    "ffd601", "ec7600", "84de00", "589a00", "50c1fb", "006fcf", "ff98e3", "f334bf", "ff2d2d",
    "af0a0a", "ffffff", "c0c0c0", "373737", "fbe7ac", "977641", "c2eaff", "fff165", "aaff7d",
];
const BASE_PROXIES: [i32; 8] = [10, 11, 12, 17, 18, 19, 21, 23];
const TEMPLATE_PROXIES: [i32; 26] = [
    3, 4, 5, 7, 13, 14, 17, 18, 21, 23, 28, 29, 30, 31, 34, 35, 40, 41, 42, 50, 51, 52, 53, 55, 61,
    67,
];

struct BadgePart {
    base: bool,
    graphic: i32,
    colour: i32,
    location: i32,
}

/// `Badge.parseBadgeData` with `isShockwaveBadge`.
fn parse_badge(code: &str) -> Vec<BadgePart> {
    let re = regex::Regex::new("[bs][0-9]{4,6}").expect("badge regex");
    re.find_iter(code)
        .map(|m| {
            let p = m.as_str();
            let num = |a: usize, b: usize| p[a..b].parse::<i32>().unwrap_or(0);
            let (graphic, colour, location) = if p.len() <= 6 {
                (
                    num(1, 3),
                    num(3, 5),
                    if p.len() > 5 { num(5, p.len()) } else { -1 },
                )
            } else {
                (num(1, 4), num(4, 6), num(6, p.len()))
            };
            BadgePart {
                base: !p.starts_with('s'),
                graphic,
                colour,
                location,
            }
        })
        .collect()
}

/// `BadgePart.getPosition`.
fn badge_position(location: i32, canvas: (i32, i32), t: &Canvas) -> (i32, i32) {
    let (cw, chh) = canvas;
    match location {
        1 => ((cw - t.w) / 2, 0),
        2 => (cw - t.w, 0),
        3 => (0, chh / 2 - t.h / 2),
        4 => ((cw - t.w) / 2, chh / 2 - t.h / 2),
        5 => (cw - t.w, chh / 2 - t.h / 2),
        6 => (0, chh - t.w),
        7 => ((cw - t.w) / 2, chh - t.h),
        8 => (cw - t.w, chh - t.h),
        _ => (0, 0),
    }
}

impl Imager {
    /// `getShockwaveTemplate`: `Err` where Java throws (no such file).
    fn shockwave_template(&self, base: bool, id: i32, proxy: bool) -> Result<Option<Canvas>, ()> {
        let mut graphic = if id < 10 {
            format!("0{id}")
        } else {
            id.to_string()
        };
        if proxy {
            graphic = format!("{graphic}_{graphic}");
        }
        let dir = if base { "base" } else { "templates" };
        let file = if id == 0 { "base".to_string() } else { graphic };
        Ok(self
            .image(&format!("badges/shockwave/{dir}/{file}.png"))?
            .map(|c| (*c).clone()))
    }

    /// `Badge.render()` for a shockwave badge; `white` is `badge-fill`'s forced background.
    pub fn badge(&self, code: &str, white: bool, gif: bool) -> Vec<u8> {
        let mut canvas = Canvas::new(39, 39);
        for part in parse_badge(code) {
            let _ = (|| -> Result<(), ()> {
                if let Some(mut t) = self.shockwave_template(part.base, part.graphic, false)? {
                    let hex = usize::try_from(part.colour - 1)
                        .ok()
                        .and_then(|i| BADGE_COLOURS.get(i))
                        .ok_or(())?;
                    t.tint(hex_to_rgb(hex).ok_or(())?, 255);
                    let (x, y) = badge_position(part.location, (39, 39), &t);
                    canvas.draw(&t, x, y);
                }
                let proxied = if part.base {
                    BASE_PROXIES.contains(&part.graphic)
                } else {
                    TEMPLATE_PROXIES.contains(&part.graphic)
                };
                if proxied
                    && let Some(t) = self.shockwave_template(part.base, part.graphic, true)?
                {
                    let (x, y) = badge_position(part.location, (39, 39), &t);
                    canvas.draw(&t, x, y);
                }
                Ok(())
            })();
        }
        if white {
            for p in canvas.px.iter_mut() {
                if *p == 0xffff_ffff {
                    *p = 0xfffe_fefe;
                } else if *p >> 24 == 0 {
                    *p = 0xffff_ffff;
                }
            }
        }
        if gif { canvas.gif() } else { canvas.png() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_match_java2d() {
        assert_eq!(mul8(255, 200), 200);
        assert_eq!(mul8(128, 200), 100);
        assert_eq!(div8(100, 128), 199);
        assert_eq!(div8(128, 128), 255);
        assert_eq!(src_over(0, 0x80c8_0000), 0x80c7_0000);
    }

    #[test]
    fn gif_round_trips_colours() {
        let mut c = Canvas::new(5, 3);
        for (i, p) in c.px.iter_mut().enumerate() {
            *p = if i % 4 == 0 {
                0
            } else {
                0xff00_0000 | (i as u32 * 0x0a0b0c)
            };
        }
        let gif = c.gif();
        assert!(gif.starts_with(b"GIF89a"));
        assert_eq!(*gif.last().unwrap(), 0x3b);
    }

    #[test]
    fn parses_shockwave_badges() {
        let p = parse_badge("b0503Xs09114s05013");
        assert_eq!(p.len(), 3);
        assert_eq!(
            (p[0].base, p[0].graphic, p[0].colour, p[0].location),
            (true, 5, 3, -1)
        );
        assert_eq!(
            (p[1].base, p[1].graphic, p[1].colour, p[1].location),
            (false, 9, 11, 4)
        );
    }
}
