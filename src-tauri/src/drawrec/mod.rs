//! Redrawing a running game from its own draw calls.
//!
//! A recording copy of the game has its Canvas draw calls pointed at a small
//! recorder (`Rec.smali`, class `gamex.Rec`) that draws as before and, while
//! GameX has it armed, notes every bitmap drawn: which one, which part of it,
//! and the full transform onto the canvas. GameX reads those notes over the
//! debugger, follows off-screen layers onto the screen, and gets the exact
//! box of every sprite on screen, each picture's pixels, and (with the
//! game's objects) which sprite is which object. Recorded while the player
//! explores, that is the level, redrawn in the editor.
//!
//! Works for every game that draws with Android's Canvas (most Java/Kotlin
//! 2D games); games drawing with OpenGL are read by their engine's reader.

pub mod level;
pub mod link;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

/// The recorder class added to the game.
pub const RECORDER: &str = include_str!("Rec.smali");
/// Its name as the debugger knows it.
pub const CLASS: &str = "gamex.Rec";

/// Records in the ring, and ints / floats per record (as in `Rec.smali`).
pub const CAP: usize = 8192;
pub const INTS: usize = 6;
pub const FLOATS: usize = 16;

/// What adding the recorder found in the game's code.
#[derive(Debug, Default, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instrumented {
    /// Files changed.
    pub files: usize,
    /// Bitmap draw calls now recorded.
    pub draws: usize,
    /// Places a frame ends (posted to the screen, or a view's onDraw).
    pub frames: usize,
}

/// Library code that never draws the game.
const SKIP: &[&str] = &["android/support/", "androidx/", "kotlin/", "kotlinx/", "com/google/", "gamex/"];

const DRAW_SIGS: &[&str] = &[
    "Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V",
    "Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Rect;Landroid/graphics/Paint;)V",
    "Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V",
    "Landroid/graphics/Bitmap;Landroid/graphics/Matrix;Landroid/graphics/Paint;)V",
];

/// `invoke-x {regs}, Target` -> (op, "{regs}", target).
fn parse_invoke(t: &str) -> Option<(&str, &str, &str)> {
    let (op, rest) = t.split_once(' ')?;
    if !op.starts_with("invoke-") {
        return None;
    }
    let close = rest.find("}, ")?;
    Some((op, &rest[..=close], &rest[close + 3..]))
}

fn static_op(op: &str) -> &'static str {
    if op.ends_with("/range") { "invoke-static/range" } else { "invoke-static" }
}

/// One smali file with its draw calls pointed at the recorder; `on_draw` also
/// starts a frame at the top of every view's onDraw(Canvas). `None` when
/// nothing in it changes. Returns the text, draws and frame points.
pub fn rewrite(text: &str, on_draw: bool) -> Option<(String, usize, usize)> {
    let mut out = String::with_capacity(text.len() + 256);
    let (mut draws, mut frames) = (0, 0);
    // Inside a view's onDraw, before its first instruction.
    let mut pending_begin = false;
    let mut annotation = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let indent = &line[..line.len() - t.len()];
        let body = t.trim_end();
        if body.starts_with(".method ") {
            pending_begin = on_draw && body.ends_with(" onDraw(Landroid/graphics/Canvas;)V") && !body.contains(" static ");
        } else if body.starts_with(".annotation") {
            annotation += 1;
        } else if body.starts_with(".end annotation") {
            annotation -= 1;
        } else if body == ".end method" {
            pending_begin = false;
        } else if pending_begin && annotation == 0 && !body.is_empty() && !body.starts_with(['.', '#', ':']) {
            out.push_str(&format!("{indent}invoke-static/range {{p1 .. p1}}, Lgamex/Rec;->begin(Landroid/graphics/Canvas;)V\n\n"));
            pending_begin = false;
            frames += 1;
        }
        let Some((op, regs, target)) = parse_invoke(body) else {
            out.push_str(line);
            continue;
        };
        let virt = op.starts_with("invoke-virtual");
        let replaced = if virt && target.starts_with("Landroid/graphics/Canvas;->drawBitmap(") {
            let sig = &target["Landroid/graphics/Canvas;->drawBitmap(".len()..];
            DRAW_SIGS.contains(&sig).then(|| {
                draws += 1;
                format!("{} {regs}, Lgamex/Rec;->drawBitmap(Landroid/graphics/Canvas;{sig}", static_op(op))
            })
        } else if virt && target == "Landroid/graphics/Canvas;->setBitmap(Landroid/graphics/Bitmap;)V" {
            Some(format!("{} {regs}, Lgamex/Rec;->setBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;)V", static_op(op)))
        } else if op.starts_with("invoke-interface") && target == "Landroid/view/SurfaceHolder;->unlockCanvasAndPost(Landroid/graphics/Canvas;)V" {
            frames += 1;
            Some(format!("{} {regs}, Lgamex/Rec;->post(Landroid/view/SurfaceHolder;Landroid/graphics/Canvas;)V", static_op(op)))
        } else if virt && target == "Landroid/view/Surface;->unlockCanvasAndPost(Landroid/graphics/Canvas;)V" {
            frames += 1;
            Some(format!("{} {regs}, Lgamex/Rec;->post(Landroid/view/Surface;Landroid/graphics/Canvas;)V", static_op(op)))
        } else {
            None
        };
        match replaced {
            Some(new) => {
                out.push_str(indent);
                out.push_str(&new);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                // A canvas drawing into a bitmap: remember which.
                if op.starts_with("invoke-direct") && target == "Landroid/graphics/Canvas;-><init>(Landroid/graphics/Bitmap;)V" {
                    if !line.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&format!(
                        "{indent}{} {regs}, Lgamex/Rec;->target(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;)V\n",
                        static_op(op)
                    ));
                }
            }
        }
    }
    (out != text).then_some((out, draws, frames))
}

fn smali_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            smali_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "smali") {
            out.push(p);
        }
    }
}

/// Adds the recorder to apktool's output at `work` (every `smali*` folder).
pub fn instrument(work: &Path) -> io::Result<Instrumented> {
    let mut files = Vec::new();
    for e in fs::read_dir(work)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() && (name == "smali" || name.starts_with("smali_")) {
            smali_files(&e.path(), &mut files);
        }
    }
    let skip = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        SKIP.iter().any(|d| s.contains(&format!("/{d}")))
    };
    files.retain(|p| !skip(p));
    // Games that post frames to a surface get frames from there; others
    // (drawn in a view) from the top of onDraw.
    let posts = files.iter().any(|p| fs::read_to_string(p).is_ok_and(|t| t.contains(";->unlockCanvasAndPost(")));
    let mut done = Instrumented::default();
    for p in &files {
        let text = fs::read_to_string(p)?;
        if let Some((new, draws, frames)) = rewrite(&text, !posts) {
            fs::write(p, new)?;
            done.files += 1;
            done.draws += draws;
            done.frames += frames;
        }
    }
    let target = work.join("smali").join("gamex").join("Rec.smali");
    fs::create_dir_all(target.parent().unwrap_or(work))?;
    fs::write(target, RECORDER)?;
    Ok(done)
}

/// The manifest with the app marked debuggable, so GameX can connect to the
/// recording copy on any device (emulators allow it anyway).
pub fn debuggable(manifest: &str) -> String {
    let Some(start) = manifest.find("<application") else { return manifest.to_string() };
    let end = manifest[start..].find('>').map(|e| start + e).unwrap_or(manifest.len());
    let tag = &manifest[start..end];
    if let Some(i) = tag.find("android:debuggable=\"") {
        let value_start = start + i + "android:debuggable=\"".len();
        let value_end = manifest[value_start..].find('"').map(|e| value_start + e).unwrap_or(value_start);
        return format!("{}true{}", &manifest[..value_start], &manifest[value_end..]);
    }
    let at = start + "<application".len();
    format!("{} android:debuggable=\"true\"{}", &manifest[..at], &manifest[at..])
}

// ---------------------------------------------------------------------------
// Reading the records

/// One record from the ring.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub kind: i32,
    pub frame: i32,
    pub canvas: i32,
    pub bitmap: i32,
    pub w: i32,
    pub h: i32,
    pub f: [f32; FLOATS],
}

/// The game's copy of the ring as GameX last read it.
#[derive(Debug, Clone)]
pub struct Ring {
    /// Records written so far (the game's `n`).
    pub n: i64,
    /// The game's frame counter.
    pub frame: i32,
    pub ints: Vec<i32>,
    pub floats: Vec<f32>,
    /// canvas -> the bitmap it draws into.
    pub targets: HashMap<i32, i32>,
}

impl Default for Ring {
    fn default() -> Self {
        Ring { n: 0, frame: 0, ints: vec![0; CAP * INTS], floats: vec![0.0; CAP * FLOATS], targets: HashMap::new() }
    }
}

impl Ring {
    /// The ring's slots written since `from` (record counts), oldest first, as
    /// (slot, how many) runs, at most one ring's worth.
    pub fn runs(from: i64, to: i64) -> Vec<(usize, usize)> {
        let from = from.max(to - CAP as i64).max(0);
        if to <= from {
            return Vec::new();
        }
        let start = (from as usize) % CAP;
        let count = (to - from) as usize;
        if start + count <= CAP {
            vec![(start, count)]
        } else {
            vec![(start, CAP - start), (0, count - (CAP - start))]
        }
    }

    /// Every record still in the ring, oldest first.
    pub fn records(&self) -> Vec<Record> {
        let count = (self.n.max(0) as usize).min(CAP);
        let first = self.n as usize - count;
        (first..self.n as usize)
            .map(|i| {
                let s = i % CAP;
                let ints = &self.ints[s * INTS..s * INTS + INTS];
                let mut f = [0f32; FLOATS];
                f.copy_from_slice(&self.floats[s * FLOATS..s * FLOATS + FLOATS]);
                Record { kind: ints[0], frame: ints[1], canvas: ints[2], bitmap: ints[3], w: ints[4], h: ints[5], f }
            })
            .collect()
    }
}

/// `x' = a x + b y + c`, `y' = d x + e y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    pub fn translate(x: f64, y: f64) -> Affine {
        Affine { c: x, f: y, ..Affine::IDENTITY }
    }

    /// From Android's matrix values (scaleX skewX transX skewY scaleY transY).
    pub fn android(v: &[f32]) -> Affine {
        Affine { a: v[0] as f64, b: v[1] as f64, c: v[2] as f64, d: v[3] as f64, e: v[4] as f64, f: v[5] as f64 }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.b * y + self.c, self.d * x + self.e * y + self.f)
    }

    /// `self` first, then `outer`.
    pub fn then(&self, outer: &Affine) -> Affine {
        Affine {
            a: outer.a * self.a + outer.b * self.d,
            b: outer.a * self.b + outer.b * self.e,
            c: outer.a * self.c + outer.b * self.f + outer.c,
            d: outer.d * self.a + outer.e * self.d,
            e: outer.d * self.b + outer.e * self.e,
            f: outer.d * self.c + outer.e * self.f + outer.f,
        }
    }

    fn sane(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite())
    }
}

/// A bitmap drawn on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Draw {
    pub bitmap: i32,
    /// The whole bitmap's size.
    pub w: i32,
    pub h: i32,
    /// The part drawn, in the bitmap's pixels: left, top, right, bottom.
    pub src: [f64; 4],
    /// Bitmap pixels -> screen pixels.
    pub to_screen: Affine,
    /// Draw order in the frame.
    pub order: usize,
}

impl Draw {
    /// Screen box: left, top, right, bottom.
    pub fn bounds(&self) -> [f64; 4] {
        let [l, t, r, b] = self.src;
        let pts = [(l, t), (r, t), (l, b), (r, b)].map(|(x, y)| self.to_screen.apply(x, y));
        let xs = pts.map(|p| p.0);
        let ys = pts.map(|p| p.1);
        [
            xs.iter().cloned().fold(f64::MAX, f64::min),
            ys.iter().cloned().fold(f64::MAX, f64::min),
            xs.iter().cloned().fold(f64::MIN, f64::max),
            ys.iter().cloned().fold(f64::MIN, f64::max),
        ]
    }

    /// What picture this draw shows: the bitmap and the part of it, rounded.
    pub fn look(&self) -> (i32, [i32; 4]) {
        (self.bitmap, self.src.map(|v| v.round() as i32))
    }
}

/// One frame of the game, as drawn on screen.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub number: i32,
    pub draws: Vec<Draw>,
    /// Bitmaps that are off-screen layers (drawn into, then onto the screen).
    pub layers: Vec<i32>,
    /// How much a layer is scaled onto the screen (1 when there's none).
    pub layer_scale: f64,
}

/// A draw's bitmap pixels -> its canvas's pixels.
fn local(r: &Record) -> Option<(Affine, [f64; 4])> {
    let full = [0.0, 0.0, r.w as f64, r.h as f64];
    let f = &r.f;
    let (m, src) = match r.kind {
        1 => (Affine::translate(f[10] as f64, f[11] as f64), full),
        2 => {
            let src = if f[6] < 0.0 { full } else { [f[6], f[7], f[8], f[9]].map(|v| v as f64) };
            let (sw, sh) = (src[2] - src[0], src[3] - src[1]);
            if sw.abs() < 1e-6 || sh.abs() < 1e-6 {
                return None;
            }
            let a = (f[12] - f[10]) as f64 / sw;
            let e = (f[13] - f[11]) as f64 / sh;
            (Affine { a, b: 0.0, c: f[10] as f64 - src[0] * a, d: 0.0, e, f: f[11] as f64 - src[1] * e }, src)
        }
        3 => (Affine::android(&f[10..16]), full),
        _ => return None,
    };
    Some((m.then(&Affine::android(&f[0..6])), src))
}

/// The latest whole frame in the ring: its draws followed onto the screen.
pub fn last_frame(ring: &Ring) -> Option<Frame> {
    let records = ring.records();
    let oldest = records.first()?.frame;
    let full_ring = ring.n as usize > CAP;
    // The newest frame that has ended and whose start is still in the ring.
    let number = records
        .iter()
        .rev()
        .map(|r| r.frame)
        .find(|&f| f < ring.frame && (!full_ring || f > oldest))?;
    let list: Vec<&Record> = records.iter().filter(|r| r.frame == number).collect();
    Some(frame_of(number, &list, &ring.targets))
}

/// Builds a frame from its records.
pub fn frame_of(number: i32, list: &[&Record], targets: &HashMap<i32, i32>) -> Frame {
    let mut screens: Vec<i32> = list.iter().filter(|r| r.kind == 8 || r.kind == 9).map(|r| r.canvas).collect();
    let draws: Vec<(&Record, Affine, [f64; 4])> =
        list.iter().filter(|r| (1..=3).contains(&r.kind)).filter_map(|r| local(r).map(|(m, s)| (*r, m, s))).collect();
    let drawn_on: std::collections::HashSet<i32> = draws.iter().map(|(r, ..)| r.canvas).collect();
    // A layer: a bitmap some canvas drew into this frame.
    let layer_of: HashMap<i32, i32> = targets.iter().filter(|(c, _)| drawn_on.contains(c)).map(|(c, b)| (*c, *b)).collect();
    let layers: std::collections::HashSet<i32> = layer_of.values().copied().collect();
    if screens.is_empty() {
        // No frame marker seen: canvases that aren't layers are the screen.
        screens = drawn_on.iter().filter(|c| !layer_of.contains_key(c)).copied().collect();
    }
    // Where each layer lands: its last draw this frame.
    let mut placed: HashMap<i32, (i32, Affine)> = HashMap::new();
    for (r, m, _) in &draws {
        if layers.contains(&r.bitmap) {
            placed.insert(r.bitmap, (r.canvas, *m));
        }
    }
    let to_screen = |canvas: i32| -> Option<Affine> {
        let mut m = Affine::IDENTITY;
        let mut c = canvas;
        for _ in 0..6 {
            if screens.contains(&c) {
                return Some(m);
            }
            let bitmap = layer_of.get(&c)?;
            let (next, onto) = placed.get(bitmap)?;
            m = m.then(onto);
            c = *next;
        }
        None
    };
    let mut layer_scale: f64 = 1.0;
    for (canvas, (_, onto)) in layer_of.iter().filter_map(|(c, b)| placed.get(b).map(|p| (c, p))) {
        if to_screen(*canvas).is_some() {
            layer_scale = onto.a.abs().max(1e-6);
        }
    }
    let mut out = Vec::new();
    for (r, m, src) in &draws {
        if layers.contains(&r.bitmap) {
            continue;
        }
        let Some(up) = to_screen(r.canvas) else { continue };
        let total = m.then(&up);
        if !total.sane() {
            continue;
        }
        out.push(Draw { bitmap: r.bitmap, w: r.w, h: r.h, src: *src, to_screen: total, order: out.len() });
    }
    Frame { number, draws: out, layers: layers.into_iter().collect(), layer_scale }
}

/// Android's ARGB pixels (not premultiplied) as RGBA bytes.
pub fn argb_to_rgba(px: &[i32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &p in px {
        let p = p as u32;
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
    }
    out
}

/// RGBA pixels as a PNG.
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().map_err(io::Error::other)?.write_image_data(rgba).map_err(io::Error::other)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_draw_calls_at_the_recorder() {
        let smali = "\
.method public paint(Landroid/graphics/Canvas;)V
    .locals 3
    invoke-virtual {p1, v0, v1, v2, v3}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V
    invoke-virtual/range {v10 .. v14}, Landroid/graphics/Canvas;->drawBitmap(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V
    invoke-virtual {p1, v0}, Landroid/graphics/Canvas;->drawColor(I)V
    new-instance v1, Landroid/graphics/Canvas;
    invoke-direct {v1, v2}, Landroid/graphics/Canvas;-><init>(Landroid/graphics/Bitmap;)V
    invoke-interface {v4, p1}, Landroid/view/SurfaceHolder;->unlockCanvasAndPost(Landroid/graphics/Canvas;)V
    return-void
.end method
";
        let (out, draws, frames) = rewrite(smali, false).unwrap();
        assert_eq!((draws, frames), (2, 1));
        assert!(out.contains("    invoke-static {p1, v0, v1, v2, v3}, Lgamex/Rec;->drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V\n"));
        assert!(out.contains("invoke-static/range {v10 .. v14}, Lgamex/Rec;->drawBitmap(Landroid/graphics/Canvas;Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;"));
        assert!(out.contains("drawColor(I)V"));
        assert!(out.contains("<init>(Landroid/graphics/Bitmap;)V\n    invoke-static {v1, v2}, Lgamex/Rec;->target("));
        assert!(out.contains("invoke-static {v4, p1}, Lgamex/Rec;->post(Landroid/view/SurfaceHolder;Landroid/graphics/Canvas;)V"));
        assert_eq!(rewrite(".method a()V\n    return-void\n.end method\n", true), None);
    }

    #[test]
    fn starts_frames_in_on_draw() {
        let smali = "\
.method protected onDraw(Landroid/graphics/Canvas;)V
    .locals 1
    .param p1, \"c\"    # Landroid/graphics/Canvas;
    .annotation build Landroidx/annotation/NonNull;
    .end annotation
    .line 12
    :cond_0
    const/4 v0, 0x0
    return-void
.end method
";
        let (out, _, frames) = rewrite(smali, true).unwrap();
        assert_eq!(frames, 1);
        let begin = out.find("Lgamex/Rec;->begin").unwrap();
        assert!(begin > out.find(":cond_0").unwrap() && begin < out.find("const/4").unwrap());
        assert!(rewrite(smali, false).is_none());
    }

    #[test]
    fn marks_the_app_debuggable() {
        assert_eq!(debuggable("<manifest><application android:label=\"x\">"), "<manifest><application android:debuggable=\"true\" android:label=\"x\">");
        assert_eq!(debuggable("<application android:debuggable=\"false\" a=\"1\">"), "<application android:debuggable=\"true\" a=\"1\">");
    }

    fn record(kind: i32, frame: i32, canvas: i32, bitmap: i32, size: (i32, i32), cm: [f32; 6], args: &[f32]) -> Record {
        let mut f = [0f32; FLOATS];
        f[..6].copy_from_slice(&cm);
        f[6] = -1.0;
        for (i, v) in args.iter().enumerate() {
            f[6 + i] = *v;
        }
        Record { kind, frame, canvas, bitmap, w: size.0, h: size.1, f }
    }

    const ID: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    #[test]
    fn follows_layers_onto_the_screen() {
        // The game draws into a 400x240 buffer (canvas 7 -> bitmap 70), then
        // the buffer onto the screen at 2x (canvas 1), then posts canvas 1.
        let targets = HashMap::from([(7, 70)]);
        let sprite = record(1, 5, 7, 11, (16, 32), ID, &[-1.0, 0.0, 0.0, 0.0, 100.0, 50.0]);
        let moved = record(1, 5, 7, 12, (8, 8), [1.0, 0.0, -20.0, 0.0, 1.0, 10.0], &[-1.0, 0.0, 0.0, 0.0, 30.0, 30.0]);
        let blit = record(2, 5, 1, 70, (400, 240), ID, &[-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 800.0, 480.0]);
        let post = record(9, 5, 1, 0, (0, 0), ID, &[]);
        let list = [&sprite, &moved, &blit, &post];
        let frame = frame_of(5, &list, &targets);
        assert_eq!(frame.draws.len(), 2);
        assert_eq!(frame.draws[0].bounds(), [200.0, 100.0, 232.0, 164.0]);
        assert_eq!(frame.draws[1].bounds(), [20.0, 80.0, 36.0, 96.0]);
        assert_eq!(frame.layers, vec![70]);
        assert_eq!(frame.layer_scale, 2.0);
        // A src rect: one 16x16 cell of a sheet.
        let cell = record(2, 5, 1, 13, (64, 64), ID, &[16.0, 0.0, 32.0, 16.0, 10.0, 10.0, 42.0, 42.0]);
        let frame = frame_of(5, &[&cell, &post], &targets);
        assert_eq!(frame.draws[0].bounds(), [10.0, 10.0, 42.0, 42.0]);
        assert_eq!(frame.draws[0].look(), (13, [16, 0, 32, 16]));
    }

    #[test]
    fn reads_the_ring_and_picks_the_last_whole_frame() {
        let mut ring = Ring::default();
        let put = |ring: &mut Ring, r: &Record| {
            let s = (ring.n as usize) % CAP;
            ring.ints[s * INTS..s * INTS + INTS].copy_from_slice(&[r.kind, r.frame, r.canvas, r.bitmap, r.w, r.h]);
            ring.floats[s * FLOATS..s * FLOATS + FLOATS].copy_from_slice(&r.f);
            ring.n += 1;
        };
        put(&mut ring, &record(1, 3, 1, 5, (4, 4), ID, &[-1.0, 0.0, 0.0, 0.0, 1.0, 1.0]));
        put(&mut ring, &record(9, 3, 1, 0, (0, 0), ID, &[]));
        put(&mut ring, &record(1, 4, 1, 6, (4, 4), ID, &[-1.0, 0.0, 0.0, 0.0, 9.0, 9.0]));
        ring.frame = 4;
        let f = last_frame(&ring).unwrap();
        assert_eq!((f.number, f.draws[0].bitmap), (3, 5));
        assert_eq!(Ring::runs(10, 12), vec![(10, 2)]);
        assert_eq!(Ring::runs(CAP as i64 - 2, CAP as i64 + 3), vec![(CAP - 2, 2), (0, 3)]);
        assert_eq!(Ring::runs(0, 3 * CAP as i64), vec![(0, CAP)]);
    }

    #[test]
    fn converts_android_pixels() {
        assert_eq!(argb_to_rgba(&[0x80112233u32 as i32]), vec![0x11, 0x22, 0x33, 0x80]);
    }
}

/// Adds the recorder to a real game's apktool output (a copy): run with
/// GX_APKTOOL=<project>/unpacked/apktool and GX_OUT=<empty folder>.
#[cfg(test)]
#[test]
#[ignore]
fn real_instrument() {
    let from = std::path::PathBuf::from(std::env::var("GX_APKTOOL").unwrap());
    let out = std::path::PathBuf::from(std::env::var("GX_OUT").unwrap());
    crate::import::copy_dir(&from, &out).unwrap();
    let done = instrument(&out).unwrap();
    println!("{done:?}");
    assert!(done.draws > 0);
}
