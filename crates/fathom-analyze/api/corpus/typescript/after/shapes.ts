// The shapes of the SDK — after the episode.

export type Area = number;

export interface Shape {
  readonly name: string;
  area(): number;
  describe(prefix: string): string;
}

export class Rect implements Shape {
  readonly name: string;
  w: number;
  h: number;
  label?: string;
  constructor(name: string, w: number, h: number) {
    this.name = name;
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
  describe(prefix: string): string {
    return `${prefix}${this.name}`;
  }
}

export class Circle implements Shape {
  readonly name: string;
  r: number;
  constructor(name: string, r: number) {
    this.name = name;
    this.r = r;
  }
  area(): number {
    return 3.14159 * this.r * this.r;
  }
  describe(prefix: string): string {
    return `${prefix}${this.name}`;
  }
}

export function distance(a: number, b: number, eps = 0): number {
  return Math.max(Math.abs(a - b), eps);
}

export function draw(target: any): string {
  return String(target);
}

/**
 * @deprecated use `draw` instead
 */
export function legacy(): string {
  return "legacy";
}

export function perimeter(w: number, h: number): number {
  return 2 * (w + h);
}

export const SIDES = 5;

export default Rect;
