/** Reading and writing Godot property values (as written in .tscn files). */

export interface Vec2 {
  x: number;
  y: number;
}

const NUMBER = "(-?\d+(?:\.\d+)?(?:e[-+]?\d+)?)";
const VECTOR2 = new RegExp(`^Vector2i?\(\s*${NUMBER}\s*,\s*${NUMBER}\s*\)$`, "i");

export function parseVector2(value: string | undefined): Vec2 | null {
  const m = value?.trim().match(VECTOR2);
  return m ? { x: Number(m[1]), y: Number(m[2]) } : null;
}

/** Up to 3 decimals, without trailing zeros: 12, 12.5, -0.125. */
export function formatNumber(n: number): string {
  const rounded = Math.round(n * 1000) / 1000;
  return Object.is(rounded, -0) ? "0" : String(rounded);
}

export function formatVector2(v: Vec2): string {
  return `Vector2(${formatNumber(v.x)}, ${formatNumber(v.y)})`;
}

export function parseNumber(value: string | undefined): number | null {
  if (value === undefined || !/^-?\d+(\.\d+)?(e[-+]?\d+)?$/i.test(value.trim())) return null;
  return Number(value);
}

/** `ExtResource("1_abc")` (Godot 4) or `ExtResource( 1 )` (Godot 3) -> the id. */
export function extResourceId(value: string | undefined): string | null {
  const m = value?.trim().match(/^ExtResource\(\s*"?([^")\s]+)"?\s*\)$/);
  return m ? m[1] : null;
}

/** A 2D affine transform: x' = a*x + c*y + e, y' = b*x + d*y + f. */
export type Matrix = [number, number, number, number, number, number];

export const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

export function multiply(m: Matrix, n: Matrix): Matrix {
  return [
    m[0] * n[0] + m[2] * n[1],
    m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3],
    m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4],
    m[1] * n[4] + m[3] * n[5] + m[5],
  ];
}

export function fromParts(position: Vec2, rotation: number, scale: Vec2): Matrix {
  const cos = Math.cos(rotation);
  const sin = Math.sin(rotation);
  return [cos * scale.x, sin * scale.x, -sin * scale.y, cos * scale.y, position.x, position.y];
}

export function apply(m: Matrix, p: Vec2): Vec2 {
  return { x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5] };
}

export function invert(m: Matrix): Matrix {
  const det = m[0] * m[3] - m[1] * m[2] || 1;
  return [
    m[3] / det,
    -m[1] / det,
    -m[2] / det,
    m[0] / det,
    (m[2] * m[5] - m[3] * m[4]) / det,
    (m[1] * m[4] - m[0] * m[5]) / det,
  ];
}
