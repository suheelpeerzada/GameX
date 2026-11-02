//! Solid shapes from triangles: a mesh collider (a house's walls and floors,
//! a cave) becomes boxes the GameX engine can stand on and bump into.
//!
//! The mesh's surface is cut into cubes (voxels) and the cubes it touches are
//! merged into as few boxes as possible: rows along x, then sheets along z,
//! then stacks along y. A flat floor becomes one thin box; a wall a few.
//! Any engine's reader can use it.

use std::collections::{BTreeMap, HashMap};

/// The most cubes a mesh is cut into along one side.
const MOST_CELLS: f32 = 400.0;
/// Roughly how many cubes the surface may touch; bigger meshes get bigger cubes.
const SURFACE_CELLS: f32 = 150_000.0;
/// The smallest cube, in metres.
const SMALLEST: f32 = 0.1;
/// About the most boxes one mesh should become; past it, it's cut again with bigger cubes.
const MOST_BOXES: usize = 1500;

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Whether a triangle touches the box at `c` with half-size `h` (separating axes).
fn touches(c: V3, h: V3, tri: [V3; 3]) -> bool {
    let v = tri.map(|p| sub(p, c));
    // The box's own axes.
    for k in 0..3 {
        let lo = v[0][k].min(v[1][k]).min(v[2][k]);
        let hi = v[0][k].max(v[1][k]).max(v[2][k]);
        if lo > h[k] || hi < -h[k] {
            return false;
        }
    }
    let e = [sub(v[1], v[0]), sub(v[2], v[1]), sub(v[0], v[2])];
    // The triangle's plane.
    let n = cross(e[0], e[1]);
    let r = h[0] * n[0].abs() + h[1] * n[1].abs() + h[2] * n[2].abs();
    if dot(n, v[0]).abs() > r {
        return false;
    }
    // Each edge crossed with each box axis.
    for edge in e {
        for k in 0..3 {
            let mut unit = [0.0; 3];
            unit[k] = 1.0;
            let a = cross(edge, unit);
            let p = v.map(|x| dot(a, x));
            let r = h[0] * a[0].abs() + h[1] * a[1].abs() + h[2] * a[2].abs();
            if p[0].min(p[1]).min(p[2]) > r || p[0].max(p[1]).max(p[2]) < -r {
                return false;
            }
        }
    }
    true
}

/// The boxes (`[min x, y, z, max x, y, z]`) that cover a mesh's surface.
/// `triangles` holds three corners (indices into `positions`) per triangle.
pub fn boxes(positions: &[V3], triangles: &[u32]) -> Vec<[f32; 6]> {
    let tris: Vec<[V3; 3]> = triangles
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| t.iter().all(|&i| (i as usize) < positions.len()))
        .map(|t| [positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]])
        .collect();
    if tris.is_empty() {
        return Vec::new();
    }
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let area: f32 = tris.iter().map(|t| {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        dot(n, n).sqrt() / 2.0
    }).sum();
    let longest = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f32::max);
    let mut size = (area / SURFACE_CELLS).sqrt().max(longest / MOST_CELLS).max(SMALLEST);
    for _ in 0..6 {
        let out = cut(&tris, lo, hi, size);
        if out.len() <= MOST_BOXES {
            return out;
        }
        size *= 1.6;
    }
    cut(&tris, lo, hi, size)
}

/// The surface cut into cubes of `size` (between `lo` and `hi`), merged into boxes.
fn cut(tris: &[[V3; 3]], lo: V3, hi: V3, size: f32) -> Vec<[f32; 6]> {
    let cells = |k: usize| (((hi[k] - lo[k]) / size).ceil() as i32).max(1);
    let n = [cells(0), cells(1), cells(2)];
    let cell_of = |x: f32, k: usize| (((x - lo[k]) / size).floor() as i32).clamp(0, n[k] - 1);
    // A hair bigger, so a wall lying exactly on a cell edge still counts.
    let half = [size / 2.0 * 1.001; 3];

    // Each cube the surface touches, with the box around the bit of surface inside it:
    // a floor at 20.0 is solid up to 20.0, not up to the top of its cubes.
    let mut filled: HashMap<(i32, i32, i32), Bounds> = HashMap::new();
    for t in tris {
        let from = [0, 1, 2].map(|k| cell_of(t[0][k].min(t[1][k]).min(t[2][k]), k));
        let to = [0, 1, 2].map(|k| cell_of(t[0][k].max(t[1][k]).max(t[2][k]), k));
        for x in from[0]..=to[0] {
            for y in from[1]..=to[1] {
                for z in from[2]..=to[2] {
                    let c = [x, y, z].map(|i| i as f32 + 0.5);
                    let c = [lo[0] + c[0] * size, lo[1] + c[1] * size, lo[2] + c[2] * size];
                    if !touches(c, half, *t) {
                        continue;
                    }
                    let Some(part) = clip(*t, sub(c, half), [c[0] + half[0], c[1] + half[1], c[2] + half[2]]) else { continue };
                    filled.entry((x, y, z)).and_modify(|b| *b = union(*b, part)).or_insert(part);
                }
            }
        }
    }

    // Neighbours a little out of line (a gentle slope) still join; a flat floor stays exact.
    let slack = size / 4.0;
    // Rows along x: (y, z) -> runs of x.
    let mut rows: BTreeMap<(i32, i32), Cells> = BTreeMap::new();
    for (&(x, y, z), &b) in &filled {
        rows.entry((y, z)).or_default().push((x, b));
    }
    // Sheets: rows with the same span, one z after another. (y, x0, x1) -> z list.
    let mut spans: BTreeMap<(i32, i32, i32), Cells> = BTreeMap::new();
    for ((y, z), xs) in rows {
        for (x0, x1, b) in runs(xs, 0, slack) {
            spans.entry((y, x0, x1)).or_default().push((z, b));
        }
    }
    // Stacks: sheets with the same rectangle, one y after another. (x0, x1, z0, z1) -> y list.
    let mut sheets: BTreeMap<(i32, i32, i32, i32), Cells> = BTreeMap::new();
    for ((y, x0, x1), zs) in spans {
        for (z0, z1, b) in runs(zs, 2, slack) {
            sheets.entry((x0, x1, z0, z1)).or_default().push((y, b));
        }
    }
    let mut out = Vec::new();
    for (_, ys) in sheets {
        for (_, _, b) in runs(ys, 1, slack) {
            out.push(b.map(round));
        }
    }
    out
}

/// `[min x, y, z, max x, y, z]`.
type Bounds = [f32; 6];

/// Cubes along one line: where each is, and the surface inside it.
type Cells = Vec<(i32, Bounds)>;

fn union(a: Bounds, b: Bounds) -> Bounds {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2]), a[3].max(b[3]), a[4].max(b[4]), a[5].max(b[5])]
}

/// The bounds of the part of a triangle inside the box `lo`..`hi`, if any of it is.
fn clip(t: [V3; 3], lo: V3, hi: V3) -> Option<Bounds> {
    let mut poly: Vec<V3> = t.to_vec();
    for k in 0..3 {
        for (edge, above) in [(lo[k], true), (hi[k], false)] {
            let inside = |p: &V3| if above { p[k] >= edge } else { p[k] <= edge };
            let mut out = Vec::with_capacity(poly.len() + 2);
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if inside(&a) {
                    out.push(a);
                }
                if inside(&a) != inside(&b) {
                    let s = (edge - a[k]) / (b[k] - a[k]);
                    let mut cut = [0, 1, 2].map(|j| a[j] + (b[j] - a[j]) * s);
                    cut[k] = edge;
                    out.push(cut);
                }
            }
            poly = out;
            if poly.is_empty() {
                return None;
            }
        }
    }
    let mut b = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
    for p in &poly {
        b = union(b, [p[0], p[1], p[2], p[0], p[1], p[2]]);
    }
    Some(b)
}

/// Cells along one line (`axis`) as runs of neighbours, each `(first, last + 1, their bounds together)`.
/// Neighbours join only when they line up (within `slack`) across the other two axes,
/// so a wall standing on a floor doesn't make the whole floor as tall as its cubes.
fn runs(mut cells: Cells, axis: usize, slack: f32) -> Vec<(i32, i32, Bounds)> {
    cells.sort_unstable_by_key(|c| c.0);
    let same = |a: &Bounds, b: &Bounds| (0..3).filter(|&k| k != axis).all(|k| (a[k] - b[k]).abs() <= slack && (a[k + 3] - b[k + 3]).abs() <= slack);
    let mut out: Vec<(i32, i32, Bounds)> = Vec::new();
    for (v, b) in cells {
        match out.last_mut() {
            Some(last) if last.1 == v && same(&last.2, &b) => {
                last.1 = v + 1;
                last.2 = union(last.2, b);
            }
            _ => out.push((v, v + 1, b)),
        }
    }
    out
}

fn round(x: f32) -> f32 {
    (x * 10000.0).round() / 10000.0 + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's surface as 12 triangles.
    fn cube(min: V3, max: V3) -> (Vec<V3>, Vec<u32>) {
        let p = |i: u32| [if i & 1 > 0 { max[0] } else { min[0] }, if i & 2 > 0 { max[1] } else { min[1] }, if i & 4 > 0 { max[2] } else { min[2] }];
        let positions = (0..8).map(p).collect();
        let faces = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];
        let tris = faces.iter().flat_map(|f| [f[0], f[1], f[2], f[0], f[2], f[3]]).collect();
        (positions, tris)
    }

    #[test]
    fn a_floor_is_one_thin_box() {
        let positions = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 10.0], [0.0, 0.0, 10.0]];
        let b = boxes(&positions, &[0, 1, 2, 0, 2, 3]);
        assert_eq!(b.len(), 1, "{b:?}");
        let [x0, y0, z0, x1, y1, z1] = b[0];
        assert!(x0 <= 0.0 && z0 <= 0.0 && x1 >= 10.0 && z1 >= 10.0);
        assert!(y0 == 0.0 && y1 == 0.0, "at the surface: {y0}..{y1}");
    }

    #[test]
    fn a_room_is_hollow() {
        // A 10 m room: someone standing in the middle is inside no box.
        let (positions, tris) = cube([0.0, 0.0, 0.0], [10.0, 4.0, 10.0]);
        let b = boxes(&positions, &tris);
        assert!(b.len() >= 6 && b.len() < 40, "{} boxes", b.len());
        let inside = |p: V3| b.iter().any(|b| (0..3).all(|k| p[k] >= b[k] - 1e-3 && p[k] <= b[k + 3] + 1e-3));
        assert!(!inside([5.0, 1.0, 5.0]));
        // The floor and the walls are solid, right at their surface.
        assert!(inside([5.0, 0.0, 5.0]));
        assert!(!inside([5.0, 0.05, 5.0]));
        assert!(inside([0.0, 2.0, 5.0]));
        assert!(inside([5.0, 2.0, 10.0]));
    }

    #[test]
    fn a_slanted_triangle_is_covered_but_not_its_bounding_box() {
        let positions = vec![[0.0, 0.0, 0.0], [4.0, 4.0, 0.0], [0.0, 4.0, 4.0]];
        let b = boxes(&positions, &[0, 1, 2]);
        let inside = |p: V3| b.iter().any(|b| (0..3).all(|k| p[k] >= b[k] && p[k] <= b[k + 3]));
        assert!(inside([0.0, 0.0, 0.0]) && inside([4.0, 4.0, 0.0]) && inside([2.0, 3.0, 1.0]));
        assert!(!inside([4.0, 0.0, 4.0]));
    }
}
