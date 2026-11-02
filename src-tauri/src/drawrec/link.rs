//! Which sprite on screen is which of the game's objects.
//!
//! The game keeps its objects' positions in its own units; the recorder gives
//! where each sprite is drawn in screen pixels. Somewhere `screen = s * game +
//! offset` holds for one corner (or the middle, or the feet) of every object's
//! sprite: votes over scale, anchor and offset find it, and then every object
//! gets the sprite drawn at its spot. The offset is the camera.

use std::collections::HashMap;

use super::Draw;

/// How game units land on the screen.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    /// Screen pixels per game unit, across and down (down may be negative).
    pub sx: f64,
    pub sy: f64,
    /// The part of the sprite at the object's position: 0 top-left, 1 middle, 2 feet.
    pub anchor: u8,
    /// Where game 0,0 is on screen: the camera.
    pub bx: f64,
    pub by: f64,
    /// Objects that sit on a sprite with this mapping.
    pub inliers: usize,
}

impl Mapping {
    pub fn to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        (self.sx * x + self.bx, self.sy * y + self.by)
    }

    pub fn to_game(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.bx) / self.sx, (y - self.by) / self.sy)
    }
}

/// A game object and where the game says it is.
#[derive(Debug, Clone)]
pub struct Obj {
    pub id: String,
    pub class: String,
    pub x: f64,
    pub y: f64,
}

pub fn anchor(b: [f64; 4], k: u8) -> (f64, f64) {
    match k {
        0 => (b[0], b[1]),
        1 => ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0),
        _ => ((b[0] + b[2]) / 2.0, b[3]),
    }
}

/// Objects worth matching: placed (not at 0,0) and not stacked on one spot
/// with many others (the game's tables).
fn usable(objects: &[Obj]) -> Vec<&Obj> {
    let mut at: HashMap<(i64, i64), usize> = HashMap::new();
    for o in objects {
        *at.entry(((o.x * 100.0) as i64, (o.y * 100.0) as i64)).or_default() += 1;
    }
    objects
        .iter()
        .filter(|o| (o.x != 0.0 || o.y != 0.0) && at[&((o.x * 100.0) as i64, (o.y * 100.0) as i64)] <= 3)
        .take(150)
        .collect()
}

/// Draws that could be objects: not the backdrop.
fn candidates(draws: &[Draw], screen: [f64; 2]) -> Vec<[f64; 4]> {
    let area = (screen[0] * screen[1]).max(1.0);
    draws
        .iter()
        .map(|d| d.bounds())
        .filter(|b| (b[2] - b[0]) * (b[3] - b[1]) < area * 0.3)
        .take(300)
        .collect()
}

/// The size of what's drawn: the union of every draw.
pub fn screen_size(draws: &[Draw]) -> [f64; 2] {
    let mut w: f64 = 1.0;
    let mut h: f64 = 1.0;
    for d in draws {
        let b = d.bounds();
        w = w.max(b[2]);
        h = h.max(b[3]);
    }
    [w, h]
}

/// The best offset for one scale/anchor: (bx, by, votes).
fn vote(objs: &[&Obj], boxes: &[[f64; 4]], sx: f64, sy: f64, k: u8) -> Option<(f64, f64, usize)> {
    // bin -> (votes, last object voting, sum x, sum y)
    let mut bins: HashMap<(i64, i64), (usize, usize, f64, f64)> = HashMap::new();
    for (i, o) in objs.iter().enumerate() {
        for b in boxes {
            let (ax, ay) = anchor(*b, k);
            let (bx, by) = (ax - sx * o.x, ay - sy * o.y);
            let e = bins.entry(((bx / 3.0).round() as i64, (by / 3.0).round() as i64)).or_insert((0, usize::MAX, 0.0, 0.0));
            if e.1 != i {
                e.0 += 1;
                e.1 = i;
                e.2 += bx;
                e.3 += by;
            }
        }
    }
    bins.into_values().max_by_key(|v| v.0).map(|(n, _, x, y)| (x / n as f64, y / n as f64, n))
}

/// Finds how the game's units map onto the screen. `prev` is tried first
/// (only the camera moved); else every likely scale is.
pub fn solve(draws: &[Draw], objects: &[Obj], prev: Option<Mapping>, layer_scale: f64) -> Option<Mapping> {
    let objs = usable(objects);
    if objs.len() < 2 {
        return None;
    }
    let boxes = candidates(draws, screen_size(draws));
    let enough = |n: usize| n >= 2 && n * 4 >= objs.len().min(12);
    if let Some(p) = prev {
        if let Some((bx, by, n)) = vote(&objs, &boxes, p.sx, p.sy, p.anchor) {
            if enough(n) && n * 2 >= p.inliers.min(objs.len()) {
                return Some(Mapping { bx, by, inliers: n, ..p });
            }
        }
    }
    let mut scales = Vec::new();
    for base in [1.0, layer_scale] {
        for k in [1.0, 2.0, 4.0, 8.0, 10.0, 16.0, 20.0, 32.0, 64.0, 100.0, 128.0, 256.0, 1000.0] {
            scales.push(base * k);
            scales.push(base / k);
        }
    }
    // Plainest first, so ties go to them: scale 1 (or the layer's), feet before corners.
    scales.sort_by(|a: &f64, b: &f64| a.ln().abs().total_cmp(&b.ln().abs()));
    scales.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut best: Option<Mapping> = None;
    for &s in &scales {
        for sy in [s, -s] {
            for k in [2u8, 1, 0] {
                if let Some((bx, by, n)) = vote(&objs, &boxes, s, sy, k) {
                    let better = best.is_none_or(|b| n > b.inliers);
                    if enough(n) && better {
                        best = Some(Mapping { sx: s, sy, anchor: k, bx, by, inliers: n });
                    }
                }
            }
        }
    }
    best
}

/// For every object, the draw (index into `draws`) showing it, if any. Each
/// draw goes to the closest object only.
pub fn assign(draws: &[Draw], objects: &[Obj], m: &Mapping) -> Vec<Option<usize>> {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    let area = {
        let s = screen_size(draws);
        s[0] * s[1]
    };
    for (oi, o) in objects.iter().enumerate() {
        if o.x == 0.0 && o.y == 0.0 {
            continue;
        }
        let (px, py) = m.to_screen(o.x, o.y);
        for (di, d) in draws.iter().enumerate() {
            let b = d.bounds();
            if (b[2] - b[0]) * (b[3] - b[1]) >= area * 0.3 {
                continue;
            }
            let (ax, ay) = anchor(b, m.anchor);
            let dist = ((ax - px).powi(2) + (ay - py).powi(2)).sqrt();
            let tol = (0.25 * (b[2] - b[0]).min(b[3] - b[1])).max(3.0);
            if dist <= tol {
                pairs.push((dist, oi, di));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = vec![None; objects.len()];
    let mut used = vec![false; draws.len()];
    for (_, oi, di) in pairs {
        if out[oi].is_none() && !used[di] {
            out[oi] = Some(di);
            used[di] = true;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The player

/// What GameX has seen of one object over time.
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub class: String,
    /// Last game position and screen position.
    pub game: (f64, f64),
    pub screen: Option<(f64, f64)>,
    /// Times it started moving right after the user pressed something.
    pub answered: u32,
    /// Times the camera moved and it stayed put on screen while moving in the game.
    pub followed: u32,
    /// Ticks seen.
    pub seen: u32,
}

const PLAYER_WORDS: &[&str] = &["player", "hero", "protagonist", "main", "avatar", "me", "self", "pj", "jugador"];

/// How sure GameX is that this object is the one the player controls.
pub fn player_score(t: &Track) -> u32 {
    let words = crate::live::words(&t.class);
    let named = words.iter().any(|w| PLAYER_WORDS.contains(&w.as_str()));
    (named as u32) * 3 + t.answered.min(6) + t.followed.min(6)
}

/// Updates the tracks with this tick's objects. `camera` is how far the camera
/// moved since the last tick; `input` whether the user pressed something
/// just before. Returns the object most likely controlled by the player.
pub fn track(
    tracks: &mut HashMap<String, Track>,
    now: &[(Obj, Option<(f64, f64)>)],
    camera: Option<(f64, f64)>,
    input: bool,
) -> Option<String> {
    let alive: std::collections::HashSet<&str> = now.iter().map(|(o, _)| o.id.as_str()).collect();
    tracks.retain(|id, _| alive.contains(id.as_str()));
    for (o, screen) in now {
        let t = tracks.entry(o.id.clone()).or_insert_with(|| Track { class: o.class.clone(), game: (o.x, o.y), ..Default::default() });
        let moved = (o.x - t.game.0).abs() + (o.y - t.game.1).abs() > 1e-6;
        if input && moved {
            t.answered += 1;
        }
        if let (Some((cx, cy)), Some(s), Some(prev)) = (camera, screen, t.screen) {
            let cam = cx.abs() + cy.abs();
            let on_screen = (s.0 - prev.0).abs() + (s.1 - prev.1).abs();
            if cam > 8.0 && moved && on_screen < cam * 0.25 {
                t.followed += 1;
            }
        }
        t.game = (o.x, o.y);
        t.screen = *screen;
        t.seen += 1;
    }
    tracks
        .iter()
        .map(|(id, t)| (player_score(t), id))
        .filter(|(s, _)| *s >= 2)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))
        .map(|(_, id)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawrec::Affine;

    fn sprite(x: f64, y: f64, w: f64, h: f64) -> Draw {
        Draw {
            bitmap: 1,
            w: w as i32,
            h: h as i32,
            src: [0.0, 0.0, w, h],
            to_screen: Affine::translate(x, y),
            order: 0,
        }
    }

    fn obj(id: &str, x: f64, y: f64) -> Obj {
        Obj { id: id.into(), class: format!("com.g.{id}"), x, y }
    }

    #[test]
    fn finds_which_sprite_is_which_object() {
        // Game units are 1/16 of a pixel; positions are the sprite's feet; the camera is at (-40, 12).
        let objects = vec![obj("hero", 1600.0, 1600.0), obj("bat", 3200.0, 800.0), obj("key", 4800.0, 2400.0), obj("table", 0.0, 0.0)];
        let sizes = [(16.0, 24.0), (20.0, 10.0), (8.0, 8.0)];
        let mut draws: Vec<Draw> = objects[..3]
            .iter()
            .zip(sizes)
            .map(|(o, (w, h))| sprite(o.x / 16.0 - 40.0 - w / 2.0, o.y / 16.0 + 12.0 - h, w, h))
            .collect();
        draws.push(sprite(0.0, 0.0, 32.0, 32.0)); // a floor tile
        draws.push(sprite(0.0, 0.0, 800.0, 480.0)); // the backdrop
        let m = solve(&draws, &objects, None, 1.0).unwrap();
        assert_eq!((m.sx, m.sy, m.anchor), (1.0 / 16.0, 1.0 / 16.0, 2));
        assert!((m.bx + 40.0).abs() < 1e-6 && (m.by - 12.0).abs() < 1e-6, "{m:?}");
        assert_eq!(assign(&draws, &objects, &m), vec![Some(0), Some(1), Some(2), None]);
        // The camera moves: the old mapping still fits with a new offset.
        let shifted: Vec<Draw> = draws.iter().map(|d| Draw { to_screen: d.to_screen.then(&Affine::translate(-100.0, 0.0)), ..d.clone() }).collect();
        let again = solve(&shifted, &objects, Some(m), 1.0).unwrap();
        assert!((again.bx + 140.0).abs() < 1e-6);
    }

    #[test]
    fn tells_the_player_by_how_it_answers_and_the_camera() {
        let mut tracks = HashMap::new();
        let tick = |tracks: &mut HashMap<String, Track>, hero: f64, bat: f64, cam: f64, input: bool| {
            let now = vec![(obj("lucy", hero, 0.0), Some((400.0, 240.0))), (obj("bat", bat, 0.0), Some((bat - cam, 100.0)))];
            track(tracks, &now, Some((cam, 0.0)), input)
        };
        assert_eq!(tick(&mut tracks, 0.0, 50.0, 0.0, false), None);
        // Lucy starts moving when the user presses, and the camera follows her.
        assert_eq!(tick(&mut tracks, 20.0, 50.0, 20.0, true).as_deref(), Some("lucy"));
        // The bat moves too, but the camera doesn't follow it.
        assert_eq!(tick(&mut tracks, 40.0, 60.0, 20.0, true).as_deref(), Some("lucy"));
        assert_eq!(player_score(&tracks["bat"]), 1);
        // A class called "player" counts from the start.
        let mut fresh = HashMap::new();
        let now = vec![(Obj { id: "7".into(), class: "com.g.PlayerShip".into(), x: 1.0, y: 1.0 }, None)];
        assert_eq!(track(&mut fresh, &now, None, false).as_deref(), Some("7"));
    }
}
