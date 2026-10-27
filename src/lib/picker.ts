// The picker's math: mapping the game's own coordinates (an object's x/y
// fields) to screen pixels, and finding the object under the pointer.

/** An object's position in the game, and where it was clicked on screen. */
export interface CalibrationPoint {
  wx: number;
  wy: number;
  sx: number;
  sy: number;
}

/** screenX = ax * x + bx, screenY = ay * y + by (device pixels). */
export interface Calibration {
  ax: number;
  bx: number;
  ay: number;
  by: number;
}

/** Least squares line through (w, s) pairs, or null without spread in w. */
function fitAxis(pairs: [number, number][]): { a: number; b: number } | null {
  const n = pairs.length;
  if (n < 2) return null;
  const mw = pairs.reduce((t, [w]) => t + w, 0) / n;
  const ms = pairs.reduce((t, [, s]) => t + s, 0) / n;
  const var_ = pairs.reduce((t, [w]) => t + (w - mw) ** 2, 0);
  const span = Math.max(...pairs.map(([w]) => w)) - Math.min(...pairs.map(([w]) => w));
  if (span < 1e-6 || var_ < 1e-9) return null;
  const a = pairs.reduce((t, [w, s]) => t + (w - mw) * (s - ms), 0) / var_;
  if (!Number.isFinite(a) || Math.abs(a) < 1e-9) return null;
  return { a, b: ms - a * mw };
}

/**
 * Solves the mapping from clicked points. An axis without spread keeps the
 * previous scale (only its offset is fitted), or borrows the other axis's
 * scale: most games scale both axes alike.
 */
export function solveCalibration(points: CalibrationPoint[], previous?: Calibration | null): Calibration | null {
  if (points.length === 0) return null;
  const fx = fitAxis(points.map((p) => [p.wx, p.sx]));
  const fy = fitAxis(points.map((p) => [p.wy, p.sy]));
  const ax = fx?.a ?? previous?.ax ?? fy?.a;
  const ay = fy?.a ?? previous?.ay ?? fx?.a;
  if (ax === undefined || ay === undefined) return null;
  const offset = (a: number, w: (p: CalibrationPoint) => number, s: (p: CalibrationPoint) => number) =>
    points.reduce((t, p) => t + s(p) - a * w(p), 0) / points.length;
  return {
    ax,
    bx: fx?.b ?? offset(ax, (p) => p.wx, (p) => p.sx),
    ay,
    by: fy?.b ?? offset(ay, (p) => p.wy, (p) => p.sy),
  };
}

export function toScreen(c: Calibration, x: number, y: number): { x: number; y: number } {
  return { x: c.ax * x + c.bx, y: c.ay * y + c.by };
}

/** Index of the point nearest to `p` within `radius`, or -1. */
export function nearest(points: { x: number; y: number }[], p: { x: number; y: number }, radius: number): number {
  let best = -1;
  let bestDistance = radius;
  points.forEach((q, i) => {
    const d = Math.hypot(q.x - p.x, q.y - p.y);
    if (d <= bestDistance) {
      best = i;
      bestDistance = d;
    }
  });
  return best;
}
