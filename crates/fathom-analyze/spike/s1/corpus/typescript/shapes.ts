import { Point } from './geo/point';
import { distance } from './geo';
import type { Handler } from './factory';
import * as Geo from './geo/point';
import './legacy';
import { sealed } from './legacy';

export type Area = number;

export const PI = 3.14159;

export enum Kind {
  Round = 'round',
  Squared = 'squared',
}

export interface Shape {
  readonly name: string;
  area(): Area;
}

export abstract class Base implements Shape {
  constructor(public readonly label: string) {}
  abstract area(): Area;
  toString(): string {
    return this.label;
  }
}

@sealed
export class Circle implements Shape {
  readonly name = 'circle';
  constructor(private r: number) {}

  area(): Area {
    return PI * this.r ** 2;
  }

  get radius(): number {
    return this.r;
  }

  static unit(p: Point): Circle {
    return new Circle(Geo.len(p) + distance(p, p));
  }
}

export function area(s: Shape): Area {
  return s.area();
}

export function parse(input: string): Shape;
export function parse(input: unknown): Shape {
  throw new Error(String(input));
}

export async function measureAll(shapes: Shape[]): Promise<Area[]> {
  return shapes.map((s) => s.area());
}

export function handlerFor(): Handler<string> {
  return (input: string) => console.log(input);
}

export default function defaultShape(): Shape {
  return new Circle(1);
}
