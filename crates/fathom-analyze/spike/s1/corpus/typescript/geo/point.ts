export namespace Geo {
  export function len(p: Point): number {
    return Math.sqrt(p.x * p.x + p.y * p.y);
  }

  export interface Vec {
    x: number;
    y: number;
  }

  export const ZERO: Point = new Point(0, 0);
}

export class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}

  norm(): number {
    return Geo.len(this);
  }
}

export type Coordinates = [number, number];

export default Point;
