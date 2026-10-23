// The shapes of the SDK: an interface, a class, and the two spellings of an optionality.

export type Area = number;

export interface Shape {
  readonly name: string;
  area(): number;
  describe(prefix?: string): string;
}

export class Rect implements Shape {
  readonly name: string;
  w: number;
  h: number;
  constructor(name: string, w: number, h: number) {
    this.name = name;
    this.w = w;
    this.h = h;
  }
  area(): number {
    return this.w * this.h;
  }
  describe(prefix?: string): string {
    return `${prefix ?? ""}${this.name}`;
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
  describe(prefix?: string): string {
    return `${prefix ?? ""}${this.name}`;
  }
}

export function distance(a: number, b: number): number {
  return Math.abs(a - b);
}

export function draw(target: Shape): string {
  return target.describe("");
}

export function gone(): number {
  return 7;
}

export const SIDES = 4;

export default Rect;
