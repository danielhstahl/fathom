import { Point } from './point';

export const Epsilon = 1e-9;

export function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y) || Epsilon;
}

export function midpoint(a: Point, b: Point): Point {
  return new Point((a.x + b.x) / 2, (a.y + b.y) / 2);
}

export function isClose(a: Point, b: Point): boolean {
  return distance(a, b) < Epsilon;
}
