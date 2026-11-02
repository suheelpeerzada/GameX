//! A recording session with a running game: each tick reads what the
//! recorder saw, fetches the pictures GameX doesn't have yet, matches sprites
//! to the game's objects, tells which one the player controls, and (while
//! exploring) adds what's on screen to the level. The level is then saved in
//! the game's layer as a scene: tiles, decor, and the game's objects as
//! things, the player as the player, enemies walking the paths they walked.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::link::{self, Mapping, Obj, Track};
use super::{last_frame, Affine, Draw, Frame, Ring, CLASS, FLOATS, INTS};
use crate::gx::model::*;
use crate::gx::project;
use crate::live::LiveSession;

/// A bitmap's pixels, as the game had them.
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// A draw on screen: (bitmap, the part of it), then where, in screen pixels.
type UiKey = ((i32, [i32; 4]), i64, i64);

#[derive(Default)]
pub struct Session {
    pub ring: Ring,
    pub sprites: HashMap<i32, Sprite>,
    /// Times GameX asked for a bitmap's pixels and didn't get them.
    tries: HashMap<i32, u8>,
    pub mapping: Option<Mapping>,
    tracks: HashMap<String, Track>,
    pub player: Option<String>,
    announced: Option<String>,
    pub last_input: Option<Instant>,
    pub paused: bool,
    pub level: Option<Level>,
    pub frame: Option<Frame>,
    /// This tick's objects, each with the draw showing it.
    pub links: Vec<(Obj, Option<usize>)>,
    /// Draws that stay put on screen while the camera moves (the HUD).
    ui: HashMap<UiKey, u32>,
    cam: Option<(f64, f64)>,
    last_targets: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxView {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// "object" (one of the game's objects), "tile", "sprite", "ui", "background".
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    pub player: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelStats {
    pub frames: u32,
    pub pieces: usize,
    pub objects: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The game has GameX's recorder (it's a recording copy).
    pub recorder: bool,
    pub frame: i32,
    pub boxes: Vec<BoxView>,
    /// The object the player controls, and its class.
    pub player: Option<(String, String)>,
    /// The player's object appeared since the last tick.
    pub spawned: bool,
    /// Sprites are matched to the game's objects.
    pub linked: bool,
    pub paused: bool,
    /// Pictures GameX has / still needs.
    pub pictures: usize,
    pub missing: usize,
    pub level: Option<LevelStats>,
}

fn int(v: Option<&crate::jdwp::Value>) -> i64 {
    match v {
        Some(crate::jdwp::Value::Int(n)) => *n,
        _ => 0,
    }
}

fn object(v: Option<&crate::jdwp::Value>) -> u64 {
    match v {
        Some(crate::jdwp::Value::Object(id)) => *id,
        _ => 0,
    }
}

/// Bitmaps bigger than this aren't fetched (backdrops are, sheets mostly are).
const MAX_PIXELS: i64 = 2048 * 2048;

impl Session {
    /// Reads what the recorder saw since the last tick. `classes` are the
    /// game's classes whose objects can be on screen.
    pub fn tick(&mut self, live: &mut LiveSession, classes: &[String], explore: bool) -> io::Result<View> {
        let Some(st) = live.statics_of(CLASS)? else { return Ok(self.view(false)) };
        if int(st.get("armed")) == 0 {
            live.set_static_int(CLASS, "armed", 1)?;
        }
        let n = int(st.get("n"));
        let (ints, floats) = (object(st.get("ints")), object(st.get("floats")));
        if ints != 0 && floats != 0 && n > self.ring.n {
            for (slot, count) in Ring::runs(self.ring.n, n) {
                let i = live.int_region(ints, slot * INTS, count * INTS)?;
                self.ring.ints[slot * INTS..(slot + count) * INTS].copy_from_slice(&i);
                let f = live.float_region(floats, slot * FLOATS, count * FLOATS)?;
                self.ring.floats[slot * FLOATS..(slot + count) * FLOATS].copy_from_slice(&f);
            }
        }
        self.ring.n = n;
        self.ring.frame = int(st.get("frame")) as i32;
        let tn = int(st.get("tn"));
        let targets = object(st.get("targets"));
        if targets != 0 && tn != self.last_targets {
            let count = (tn.min(1024) * 2) as usize;
            let pairs = live.int_region(targets, 0, count)?;
            // Oldest first, so later canvases for a bitmap win.
            let start = if tn > 1024 { (tn % 1024) as usize } else { 0 };
            self.ring.targets.clear();
            for k in 0..count / 2 {
                let s = (start + k) % (count / 2);
                self.ring.targets.insert(pairs[s * 2], pairs[s * 2 + 1]);
            }
            self.last_targets = tn;
        }
        if let Some(f) = last_frame(&self.ring) {
            self.frame = Some(f);
        }
        if !self.paused {
            self.fetch(live)?;
        }
        let positions = live.positions(classes, 400)?;
        let objs: Vec<Obj> = positions.into_iter().map(|p| Obj { id: p.object, class: p.class, x: p.x, y: p.y }).collect();
        let draws: &[Draw] = self.frame.as_ref().map(|f| f.draws.as_slice()).unwrap_or(&[]);
        let layer_scale = self.frame.as_ref().map(|f| f.layer_scale).unwrap_or(1.0);
        self.mapping = link::solve(draws, &objs, self.mapping, layer_scale).or(self.mapping);
        let assigned = match &self.mapping {
            Some(m) => link::assign(draws, &objs, m),
            None => vec![None; objs.len()],
        };
        // The camera: where game 0,0 is on screen.
        let cam = self.mapping.map(|m| (m.bx, m.by));
        let moved = match (cam, self.cam) {
            (Some(now), Some(before)) => Some((now.0 - before.0, now.1 - before.1)),
            _ => None,
        };
        if cam.is_some() {
            self.cam = cam;
        }
        if moved.is_some_and(|(dx, dy)| dx.abs() + dy.abs() > 8.0) {
            // What stayed put on screen while the camera moved is the HUD.
            for d in draws {
                let b = d.bounds();
                *self.ui.entry((d.look(), b[0].round() as i64, b[1].round() as i64)).or_default() += 1;
            }
        }
        let with_screen: Vec<(Obj, Option<(f64, f64)>)> = objs
            .iter()
            .zip(&assigned)
            .map(|(o, a)| (o.clone(), a.map(|i| link::anchor(draws[i].bounds(), 1))))
            .collect();
        let input = self.last_input.is_some_and(|t| t.elapsed() < Duration::from_millis(900));
        self.player = link::track(&mut self.tracks, &with_screen, moved, input);
        self.links = objs.into_iter().zip(assigned).collect();
        if explore && !self.paused {
            let ui = self.ui_draws();
            let level = self.level.get_or_insert_with(Level::default);
            if let Some(f) = &self.frame {
                level.add(f, &self.links, self.cam.unwrap_or((0.0, 0.0)), &ui, self.player.as_deref());
            }
        }
        Ok(self.view(true))
    }

    /// Indices of this frame's draws that are the HUD.
    fn ui_draws(&self) -> HashSet<usize> {
        let Some(f) = &self.frame else { return HashSet::new() };
        f.draws
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                let b = d.bounds();
                self.ui.get(&(d.look(), b[0].round() as i64, b[1].round() as i64)).copied().unwrap_or(0) >= 2
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Asks the game for the pixels of bitmaps on screen GameX doesn't have,
    /// for up to about half a second.
    fn fetch(&mut self, live: &mut LiveSession) -> io::Result<()> {
        let Some(f) = &self.frame else { return Ok(()) };
        let layers: HashSet<i32> = f.layers.iter().copied().collect();
        let mut wanted: Vec<i32> = Vec::new();
        for d in &f.draws {
            let ok = (d.w as i64) * (d.h as i64) <= MAX_PIXELS && d.w > 0 && d.h > 0;
            if ok && !layers.contains(&d.bitmap) && !self.sprites.contains_key(&d.bitmap) && self.tries.get(&d.bitmap).copied().unwrap_or(0) < 3 && !wanted.contains(&d.bitmap) {
                wanted.push(d.bitmap);
            }
        }
        let started = Instant::now();
        for key in wanted {
            if started.elapsed() > Duration::from_millis(500) {
                break;
            }
            live.set_static_int(CLASS, "want", key)?;
            let asked = Instant::now();
            let mut got = false;
            while asked.elapsed() < Duration::from_millis(160) {
                std::thread::sleep(Duration::from_millis(12));
                let Some(st) = live.statics_of(CLASS)? else { break };
                if int(st.get("got")) as i32 == key {
                    let (w, h) = (int(st.get("pw")) as u32, int(st.get("ph")) as u32);
                    let pixels = object(st.get("pixels"));
                    if pixels != 0 && w > 0 && h > 0 {
                        let px = live.int_region(pixels, 0, (w * h) as usize)?;
                        self.sprites.insert(key, Sprite { w, h, rgba: super::argb_to_rgba(&px) });
                        got = true;
                    }
                    break;
                }
            }
            if !got {
                // Not drawn again in time (or not readable): tried again on a later tick, twice.
                *self.tries.entry(key).or_default() += 1;
            }
        }
        live.set_static_int(CLASS, "want", 0)?;
        Ok(())
    }

    fn view(&mut self, recorder: bool) -> View {
        let draws: &[Draw] = self.frame.as_ref().map(|f| f.draws.as_slice()).unwrap_or(&[]);
        let screen = link::screen_size(draws);
        let area = screen[0] * screen[1];
        let mut by_draw: HashMap<usize, &Obj> = HashMap::new();
        for (o, d) in &self.links {
            if let Some(d) = d {
                by_draw.insert(*d, o);
            }
        }
        let mut uses: HashMap<i32, usize> = HashMap::new();
        for d in draws {
            *uses.entry(d.bitmap).or_default() += 1;
        }
        let ui = self.ui_draws();
        let boxes = draws
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let b = d.bounds();
                let o = by_draw.get(&i);
                let kind = if o.is_some() {
                    "object"
                } else if (b[2] - b[0]) * (b[3] - b[1]) >= area * 0.3 {
                    "background"
                } else if ui.contains(&i) {
                    "ui"
                } else if uses[&d.bitmap] >= 3 {
                    "tile"
                } else {
                    "sprite"
                };
                BoxView {
                    x: b[0],
                    y: b[1],
                    w: b[2] - b[0],
                    h: b[3] - b[1],
                    kind,
                    object: o.map(|o| o.id.clone()),
                    class: o.map(|o| o.class.clone()),
                    player: o.is_some_and(|o| Some(&o.id) == self.player.as_ref()),
                }
            })
            .collect();
        let player = self.player.as_ref().and_then(|id| self.tracks.get(id).map(|t| (id.clone(), t.class.clone())));
        let spawned = self.player.is_some() && self.player != self.announced;
        if spawned {
            self.announced = self.player.clone();
        }
        let missing = draws.iter().map(|d| d.bitmap).collect::<HashSet<_>>().iter().filter(|b| !self.sprites.contains_key(b)).count();
        View {
            recorder,
            frame: self.frame.as_ref().map(|f| f.number).unwrap_or(0),
            boxes,
            player,
            spawned,
            linked: self.mapping.is_some(),
            paused: self.paused,
            pictures: self.sprites.len(),
            missing,
            level: self.level.as_ref().map(|l| LevelStats { frames: l.frames, pieces: l.kept().len(), objects: l.actors.len() }),
        }
    }
}

// ---------------------------------------------------------------------------
// The level, as explored

/// A picture that stays put in the level.
#[derive(Debug, Clone)]
pub struct Piece {
    pub draw: Draw,
    /// Bitmap pixels -> level pixels.
    pub world: Affine,
    pub seen: u32,
    pub last: u32,
}

/// One of the game's objects, as seen.
#[derive(Debug, Clone)]
pub struct Actor {
    pub class: String,
    pub draw: Draw,
    pub world: Affine,
    /// Where it has been (the middle of its sprite, level pixels).
    pub path: Vec<(f64, f64)>,
    pub player: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Level {
    pub frames: u32,
    pieces: HashMap<(i32, i64, i64), Piece>,
    pub actors: BTreeMap<String, Actor>,
    /// Where the player's feet have been (level pixels).
    pub feet: Vec<(f64, f64)>,
    /// The screen's size, for the camera.
    pub screen: [f64; 2],
}

impl Level {
    /// Adds a frame. `cam` is where level 0,0 is on screen; `ui` the HUD's draws.
    pub fn add(&mut self, frame: &Frame, links: &[(Obj, Option<usize>)], cam: (f64, f64), ui: &HashSet<usize>, player: Option<&str>) {
        self.frames += 1;
        let back = Affine::translate(-cam.0, -cam.1);
        let size = link::screen_size(&frame.draws);
        self.screen = [self.screen[0].max(size[0]), self.screen[1].max(size[1])];
        let mut linked = HashSet::new();
        for (o, d) in links {
            let Some(i) = d else { continue };
            linked.insert(*i);
            let draw = &frame.draws[*i];
            let world = draw.to_screen.then(&back);
            let b = world_bounds(draw, &world);
            let mid = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
            let is_player = player == Some(o.id.as_str());
            if is_player {
                let feet = (mid.0, b[3] - (b[3] - b[1]) * 0.1);
                if self.feet.last().is_none_or(|l| (l.0 - feet.0).abs() + (l.1 - feet.1).abs() > 2.0) {
                    self.feet.push(feet);
                }
            }
            let a = self.actors.entry(o.id.clone()).or_insert_with(|| Actor { class: o.class.clone(), draw: draw.clone(), world, path: Vec::new(), player: false });
            a.draw = draw.clone();
            a.world = world;
            a.player |= is_player;
            if a.path.last().is_none_or(|l| (l.0 - mid.0).abs() + (l.1 - mid.1).abs() > 2.0) && a.path.len() < 400 {
                a.path.push(mid);
            }
        }
        for (i, d) in frame.draws.iter().enumerate() {
            if linked.contains(&i) || ui.contains(&i) {
                continue;
            }
            let world = d.to_screen.then(&back);
            let b = world_bounds(d, &world);
            let key = (d.bitmap, b[0].round() as i64, b[1].round() as i64);
            let p = self.pieces.entry(key).or_insert_with(|| Piece { draw: d.clone(), world, seen: 0, last: 0 });
            p.seen += 1;
            p.last = self.frames;
            p.draw.order = d.order;
        }
    }

    /// Pieces that are part of the level: seen a few times, or still there now.
    pub fn kept(&self) -> Vec<&Piece> {
        let mut out: Vec<&Piece> = self.pieces.values().filter(|p| p.seen >= 3 || p.last == self.frames).collect();
        out.sort_by_key(|p| p.draw.order);
        out
    }
}

fn world_bounds(d: &Draw, world: &Affine) -> [f64; 4] {
    Draw { to_screen: *world, ..d.clone() }.bounds()
}

// ---------------------------------------------------------------------------
// Saving the level as a scene

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub scene: String,
    pub name: String,
    pub tiles: usize,
    pub sprites: usize,
    pub objects: usize,
    /// The class of the object the player controls.
    pub player: Option<String>,
    /// Pictures GameX never got (shown as boxes).
    pub missing: usize,
}

/// The part of a sprite a draw shows, as RGBA.
fn crop(s: &Sprite, src: [f64; 4]) -> (u32, u32, Vec<u8>) {
    let l = src[0].max(0.0).round() as u32;
    let t = src[1].max(0.0).round() as u32;
    let r = (src[2].round() as u32).min(s.w).max(l + 1);
    let b = (src[3].round() as u32).min(s.h).max(t + 1);
    let (w, h) = (r - l, b - t);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (l + x, t + y);
            if sx < s.w && sy < s.h {
                let si = ((sy * s.w + sx) * 4) as usize;
                let di = ((y * w + x) * 4) as usize;
                out[di..di + 4].copy_from_slice(&s.rgba[si..si + 4]);
            }
        }
    }
    (w, h, out)
}

fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Saves a picture under the layer's assets (once per content); returns its asset path.
fn save_picture(dir: &Path, folder: &str, w: u32, h: u32, rgba: &[u8]) -> io::Result<String> {
    let asset = format!("{folder}/{}-{w}x{h}.png", hash(rgba));
    let file = dir.join("assets").join(&asset);
    if !file.is_file() {
        fs::create_dir_all(file.parent().unwrap_or(dir))?;
        fs::write(&file, super::encode_png(w, h, rgba)?)?;
    }
    Ok(asset)
}

fn round3(v: f64) -> f32 {
    ((v * 1000.0).round() / 1000.0) as f32
}

/// The tile grid most of the pieces sit on: cell size and offset (level pixels).
fn grid(pieces: &[&Piece]) -> Option<([f64; 2], [f64; 2])> {
    let mut sizes: HashMap<(i64, i64), usize> = HashMap::new();
    for p in pieces {
        if p.world.b.abs() < 1e-6 && p.world.d.abs() < 1e-6 {
            let b = world_bounds(&p.draw, &p.world);
            *sizes.entry(((b[2] - b[0]).round() as i64, (b[3] - b[1]).round() as i64)).or_default() += 1;
        }
    }
    let ((cw, ch), n) = sizes.into_iter().filter(|((w, h), _)| *w >= 4 && *h >= 4).max_by_key(|(_, n)| *n)?;
    if n < 8 {
        return None;
    }
    let mut offsets: HashMap<(i64, i64), usize> = HashMap::new();
    for p in pieces {
        let b = world_bounds(&p.draw, &p.world);
        if (b[2] - b[0]).round() as i64 == cw && (b[3] - b[1]).round() as i64 == ch {
            *offsets.entry(((b[0].round() as i64).rem_euclid(cw), (b[1].round() as i64).rem_euclid(ch))).or_default() += 1;
        }
    }
    let ((ox, oy), _) = offsets.into_iter().max_by_key(|(_, n)| *n)?;
    Some(([cw as f64, ch as f64], [ox as f64, oy as f64]))
}

fn inside(b: &[f64; 4], p: &(f64, f64)) -> bool {
    p.0 >= b[0] && p.0 <= b[2] && p.1 >= b[1] && p.1 <= b[3]
}

/// Saves the explored level in the layer at `dir` as a new scene. `kinds`
/// gives each class's category in GameX ("characters", "items"...).
pub fn save(dir: &Path, level: &Level, sprites: &HashMap<i32, Sprite>, kinds: &HashMap<String, String>) -> io::Result<Saved> {
    let index = project::open(dir)?;
    let ppu = index.info.settings.pixels_per_unit.max(1.0) as f64;
    let source = index.info.sources.first().map(|s| s.id.clone()).unwrap_or_else(|| "game".into());
    let folder = format!("textures/{source}/redrawn");
    let m = |v: f64| round3(v / ppu);
    let mut missing: HashSet<i32> = HashSet::new();

    let pieces = level.kept();
    let screen_area = (level.screen[0] * level.screen[1]).max(1.0);
    let cell = grid(&pieces);
    let walked = |b: &[f64; 4]| level.feet.iter().any(|f| inside(b, f));
    let near = |b: &[f64; 4], r: f64| level.feet.iter().any(|f| inside(&[b[0] - r, b[1] - r, b[2] + r, b[3] + r], f));

    // Tiles: pieces on the grid, one atlas of their pictures.
    let mut atlas_index: HashMap<(i32, [i32; 4]), u32> = HashMap::new();
    let mut atlas_tiles: Vec<Vec<u8>> = Vec::new();
    let mut cells: Vec<[i32; 6]> = Vec::new();
    let mut walkable: HashSet<u32> = HashSet::new();
    let mut decor: Vec<&Piece> = Vec::new();
    for p in &pieces {
        let b = world_bounds(&p.draw, &p.world);
        let on_grid = cell.is_some_and(|([cw, ch], [ox, oy])| {
            ((b[2] - b[0]) - cw).abs() < 1.0
                && ((b[3] - b[1]) - ch).abs() < 1.0
                && ((b[0] - ox).rem_euclid(cw) < 1.0 || (b[0] - ox).rem_euclid(cw) > cw - 1.0)
                && ((b[1] - oy).rem_euclid(ch) < 1.0 || (b[1] - oy).rem_euclid(ch) > ch - 1.0)
                && p.world.b.abs() < 1e-6
                && p.world.d.abs() < 1e-6
        });
        let Some(([cw, ch], [ox, oy])) = cell.filter(|_| on_grid) else {
            decor.push(p);
            continue;
        };
        let look = p.draw.look();
        let next = atlas_index.len() as u32;
        let idx = *atlas_index.entry(look).or_insert_with(|| {
            let (tw, th) = (cw as u32, ch as u32);
            let tile = match sprites.get(&p.draw.bitmap) {
                Some(s) => {
                    let (w, h, px) = crop(s, p.draw.src);
                    // Scaled to the cell (nearest), as the game draws it.
                    let mut out = vec![0u8; (tw * th * 4) as usize];
                    for y in 0..th {
                        for x in 0..tw {
                            let sx = (x * w / tw).min(w - 1);
                            let sy = (y * h / th).min(h - 1);
                            let si = ((sy * w + sx) * 4) as usize;
                            let di = ((y * tw + x) * 4) as usize;
                            out[di..di + 4].copy_from_slice(&px[si..si + 4]);
                        }
                    }
                    out
                }
                None => {
                    missing.insert(p.draw.bitmap);
                    [0x7au8, 0x8c, 0xff, 0x90].repeat((tw * th) as usize)
                }
            };
            atlas_tiles.push(tile);
            next
        });
        if walked(&b) {
            walkable.insert(idx);
        }
        let x = ((b[0] - ox) / cw).round() as i32;
        let y = -((b[1] - oy) / ch).round() as i32 - 1;
        let flags = (p.world.a < 0.0) as i32 | (((p.world.e < 0.0) as i32) << 1);
        const COLS: u32 = 16;
        cells.push([x, y, 0, (idx % COLS) as i32, (idx / COLS) as i32, flags]);
    }

    let mut entities = Vec::new();
    let mut tiles_count = 0;
    if let Some(([cw, ch], [ox, oy])) = cell.filter(|_| !atlas_tiles.is_empty()) {
        const COLS: u32 = 16;
        let (tw, th) = (cw as u32, ch as u32);
        let n = atlas_tiles.len() as u32;
        let rows = n.div_ceil(COLS);
        let (aw, ah) = (tw * COLS.min(n), th * rows);
        let mut atlas = vec![0u8; (aw * ah * 4) as usize];
        for (i, tile) in atlas_tiles.iter().enumerate() {
            let (cx, cy) = ((i as u32 % COLS) * tw, (i as u32 / COLS) * th);
            for y in 0..th {
                let di = (((cy + y) * aw + cx) * 4) as usize;
                let si = (y * tw * 4) as usize;
                atlas[di..di + (tw * 4) as usize].copy_from_slice(&tile[si..si + (tw * 4) as usize]);
            }
        }
        let asset = save_picture(dir, &folder, aw, ah, &atlas)?;
        // Tiles the player never stood on are walls, once GameX has seen where they walked.
        let solid: Vec<[u32; 2]> = if level.feet.len() >= 4 {
            (0..n).filter(|i| !walkable.contains(i)).map(|i| [i % COLS, i / COLS]).collect()
        } else {
            Vec::new()
        };
        tiles_count = cells.len();
        let mut e = Entity::new("level-tiles", "Level tiles");
        e.transform.position = [m(ox), -m(oy), -0.2];
        e.parts.push(Part::Tiles {
            cell: [m(cw), m(ch)],
            sheets: vec![TileSheet { asset, tile: [tw, th], margin: [0, 0], separation: [0, 0], solid }],
            cells,
        });
        entities.push(e);
    }

    // Everything else that stays put: decor (and the backdrop), in drawing order.
    let mut group = Entity::new("decor", "Decor");
    let count = decor.len().max(1) as f64;
    for (i, p) in decor.iter().enumerate() {
        let b = world_bounds(&p.draw, &p.world);
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        let backdrop = w * h >= screen_area * 0.3;
        let mut e = Entity::new(&format!("decor-{}", i + 1), if backdrop { "Backdrop" } else { "Decor" });
        let z = if backdrop { -0.45 } else { -0.15 + 0.1 * (i as f64 / count) };
        e.transform.position = [m((b[0] + b[2]) / 2.0), -m((b[1] + b[3]) / 2.0), z as f32];
        match sprites.get(&p.draw.bitmap) {
            Some(s) => {
                let (cw, chh, px) = crop(s, p.draw.src);
                let asset = save_picture(dir, &folder, cw, chh, &px)?;
                e.parts.push(Part::Sprite { asset, frame: None, size: Some([m(w), m(h)]), flip_x: p.world.a < 0.0, frames: Vec::new(), fps: None });
            }
            None => {
                missing.insert(p.draw.bitmap);
                e.parts.push(Part::Shape { shape: ShapeKind::Box, size: [m(w), m(h), 0.05], color: "#7a8cff".into() });
            }
        }
        // Furniture the player walked around, never through, blocks.
        if !backdrop && level.feet.len() >= 4 && !walked(&b) && near(&b, w.max(h).max(16.0)) {
            e.parts.push(Part::Collider { shape: ShapeKind::Box, size: [m(w), m(h), 0.5], body: Body::Static, trigger: false, boxes: Vec::new() });
        } else {
            e.tags.push("no-collide".into());
        }
        group.children.push(e);
    }
    let sprites_count = group.children.len();
    if !group.children.is_empty() {
        entities.push(group);
    }

    // The game's objects, as things.
    let mut things: Vec<Thing> = index.things.iter().filter_map(|t| project::load_thing(dir, &t.id).ok()).collect();
    let mut objects = Entity::new("objects", "Objects");
    let mut player_class = None;
    let mut taken: HashSet<String> = HashSet::new();
    for (oid, a) in &level.actors {
        let simple = a.class.rsplit(['.', '$']).next().unwrap_or(&a.class).to_string();
        let b = world_bounds(&a.draw, &a.world);
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        let category = kinds.get(&a.class).map(String::as_str).unwrap_or("");
        let role = if a.player {
            Some("player")
        } else if category == "characters" {
            Some("enemy")
        } else {
            None
        };
        let thing_id = match crate::gx::generic::thing_for_class(&a.class, &things).map(|t| t.id.clone()) {
            Some(id) => {
                // The thing exists (with the game's art); it learns its role.
                if let Some(t) = things.iter_mut().find(|t| t.id == id) {
                    if t.role.is_none() && role.is_some() {
                        t.role = role.map(String::from);
                        project::save_thing(dir, t, &format!("{} plays the {}", t.name, role.unwrap_or("")))?;
                    }
                }
                id
            }
            None => {
                let id = project::slug(&simple);
                let id = if id.is_empty() { "object".to_string() } else { id };
                let mut parts = Vec::new();
                match sprites.get(&a.draw.bitmap) {
                    Some(s) => {
                        let (cw, chh, px) = crop(s, a.draw.src);
                        let asset = save_picture(dir, &folder, cw, chh, &px)?;
                        parts.push(Part::Sprite { asset, frame: None, size: Some([m(w), m(h)]), flip_x: false, frames: Vec::new(), fps: None });
                    }
                    None => {
                        missing.insert(a.draw.bitmap);
                        parts.push(Part::Shape { shape: ShapeKind::Box, size: [m(w), m(h), 0.05], color: "#7a8cff".into() });
                    }
                }
                let kind = match category {
                    "characters" => ThingKind::Character,
                    "items" => ThingKind::Item,
                    _ => ThingKind::Prop,
                };
                let t = Thing {
                    format: FORMAT,
                    id: id.clone(),
                    name: simple.clone(),
                    kind,
                    role: role.map(String::from),
                    description: Some(format!("Redrawn from the running game ({}).", a.class)),
                    properties: Vec::new(),
                    parts,
                    behaviors: Vec::new(),
                    children: Vec::new(),
                    tags: Vec::new(),
                };
                project::save_thing(dir, &t, &format!("Add {simple} from the running game"))?;
                things.push(t);
                id
            }
        };
        let mut eid = format!("{thing_id}-1");
        let mut k = 2;
        while !taken.insert(eid.clone()) {
            eid = format!("{thing_id}-{k}");
            k += 1;
        }
        let mut e = Entity::new(&eid, &simple);
        e.thing = Some(thing_id);
        e.transform.position = [m((b[0] + b[2]) / 2.0), -m((b[1] + b[3]) / 2.0), 0.0];
        if a.draw.to_screen.a < 0.0 {
            e.transform.scale[0] = -1.0;
        }
        let mut values = BTreeMap::new();
        values.insert("class".into(), Value::Text(a.class.clone()));
        values.insert("object".into(), Value::Text(oid.clone()));
        e.parts.push(Part::Custom { name: "Game object".into(), values });
        if a.player {
            player_class = Some(a.class.clone());
            // The camera follows the player, a screen high.
            e.parts.push(Part::Camera { fov: None, ortho_size: Some(m(level.screen[1] / 2.0)) });
        } else if a.path.len() >= 3 {
            // Where it went in the game: it walks that way again.
            let (min_x, max_x) = a.path.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
            let (min_y, max_y) = a.path.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
            if max_x - min_x + max_y - min_y > 16.0 {
                let points: Vec<String> = a.path.iter().map(|p| format!("{},{}", m(p.0), -m(p.1))).collect();
                let mut values = BTreeMap::new();
                values.insert("points".into(), Value::Text(points.join(";")));
                e.parts.push(Part::Custom { name: "Recorded path".into(), values });
            }
        }
        objects.children.push(e);
    }
    let objects_count = objects.children.len();
    if !objects.children.is_empty() {
        entities.push(objects);
    }

    let base = "redrawn";
    let mut id = base.to_string();
    let mut n = 2;
    while index.scenes.iter().any(|s| s.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let name = if id == base { "Redrawn level".to_string() } else { format!("Redrawn level {}", n - 1) };
    let scene = Scene {
        format: FORMAT,
        id: id.clone(),
        name: name.clone(),
        dimensions: Dimensions::TwoD,
        environment: Environment { background: "#000000".into(), ..Environment::default() },
        entities,
    };
    project::save_scene(dir, &scene, &format!("Redraw {name} from the running game"))?;
    Ok(Saved { scene: id, name, tiles: tiles_count, sprites: sprites_count, objects: objects_count, player: player_class, missing: missing.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(bitmap: i32, x: f64, y: f64, w: f64, h: f64, order: usize) -> Draw {
        Draw { bitmap, w: w as i32, h: h as i32, src: [0.0, 0.0, w, h], to_screen: Affine::translate(x, y), order }
    }

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Sprite {
        Sprite { w, h, rgba: rgba.repeat((w * h) as usize) }
    }

    #[test]
    fn explores_and_saves_a_level() {
        let root = std::env::temp_dir().join(format!("gamex-redraw-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = project::create(&root, "Redraw test").unwrap();

        // A 10x3 room of 32px tiles: floor (bitmap 1) in the middle row, walls (2) around.
        let mut draws = Vec::new();
        for y in 0..3 {
            for x in 0..10 {
                draws.push(draw(if y == 1 { 1 } else { 2 }, x as f64 * 32.0, y as f64 * 32.0, 32.0, 32.0, draws.len()));
            }
        }
        draws.push(draw(3, 100.0, 40.0, 20.0, 24.0, draws.len())); // the hero, on the floor row
        draws.push(draw(4, 250.0, 40.0, 16.0, 16.0, draws.len())); // a bat
        let hero = Obj { id: "1".into(), class: "com.g.hero".into(), x: 110.0, y: 64.0 };
        let bat = Obj { id: "2".into(), class: "com.g.bat".into(), x: 258.0, y: 56.0 };
        let mut level = Level::default();
        for step in 0..5 {
            let mut f = Frame { number: step, draws: draws.clone(), layers: vec![], layer_scale: 1.0 };
            // The hero walks along the floor; the bat flies right.
            f.draws[30].to_screen = Affine::translate(100.0 + step as f64 * 20.0, 40.0);
            f.draws[31].to_screen = Affine::translate(250.0 + step as f64 * 6.0, 40.0);
            let links = vec![(hero.clone(), Some(30)), (bat.clone(), Some(31))];
            level.add(&f, &links, (0.0, 0.0), &HashSet::new(), Some("1"));
        }
        assert_eq!(level.kept().len(), 30);
        assert_eq!(level.actors.len(), 2);
        let sprites = HashMap::from([
            (1, solid(32, 32, [90, 60, 40, 255])),
            (2, solid(32, 32, [40, 120, 120, 255])),
            (3, solid(20, 24, [200, 80, 120, 255])),
        ]);
        let kinds = HashMap::from([("com.g.hero".to_string(), "characters".to_string()), ("com.g.bat".to_string(), "characters".to_string())]);
        let saved = save(&dir, &level, &sprites, &kinds).unwrap();
        assert_eq!((saved.tiles, saved.objects, saved.missing), (30, 2, 1));
        assert_eq!(saved.player.as_deref(), Some("com.g.hero"));
        let scene = project::load_scene(&dir, &saved.scene).unwrap();
        let Part::Tiles { sheets, cells, .. } = &scene.entities[0].parts[0] else { panic!("tiles first") };
        // Two pictures; the wall (first drawn, never walked on) is solid.
        assert_eq!(sheets[0].solid, vec![[0, 0]]);
        assert_eq!(cells.len(), 30);
        let hero_thing = project::load_thing(&dir, "hero").unwrap();
        assert_eq!(hero_thing.role.as_deref(), Some("player"));
        assert_eq!(project::load_thing(&dir, "bat").unwrap().role.as_deref(), Some("enemy"));
        let objects = scene.entities.iter().find(|e| e.id == "objects").unwrap();
        let bat_entity = objects.children.iter().find(|e| e.thing.as_deref() == Some("bat")).unwrap();
        assert!(bat_entity.parts.iter().any(|p| matches!(p, Part::Custom { name, .. } if name == "Recorded path")));
        let _ = fs::remove_dir_all(&root);
    }
}
