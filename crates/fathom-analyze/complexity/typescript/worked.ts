// ADR-007 §4.3's list, in the shapes a reviewer meets: the arms, the three spellings of "maybe",
// and the four loops.
//
// The signature kinds are units with no body in this file — `body_not_visible`, a `?` about this file.

export interface Shape {
  area(): number;
  describe(): string;
}

export abstract class Figure implements Shape {
  constructor(protected sides: number) {}

  abstract area(): number;

  describe(): string {
    if (this.sides <= 0 || Number.isNaN(this.sides)) {
      return "none";
    }
    return this.sides > 3 ? "polygon" : "triangle";
  }
}

export function classify(values: number[], label?: string): string {
  switch (values.length) {
    case 0:
    case 1:
      return "few";
    case 2:
      // `&&` is a point of its own, and it is the operator a counter misses when it counts node
      // kinds instead of the operator's spelling: in this grammar the spelling is an anonymous token
      // inside `binary_expression`, and counting the host would count every `<` in the file.
      if (values.length > 2 && label) {
        return label;
      }
      return "two";
    default:
      return label?.toUpperCase() ?? "many";
  }
}

export function loops(rows: Array<{ ok: boolean }>, keys: string[]): number {
  let seen = 0;
  for (const row of rows) {
    if (row.ok) {
      seen += 1;
    }
  }
  for (const key of keys) {
    if (key) {
      seen += key.length;
    }
  }
  for (let index = 0; index < keys.length; index += 1) {
    seen += index;
  }
  for (const key in keys) {
    seen += Number(key);
  }
  while (seen > 100) {
    seen -= 100;
  }
  do {
    seen -= 1;
  } while (seen < 0);
  return seen;
}

export function guarded(input: string): string {
  try {
    if (!input) {
      throw new Error("empty");
    }
    return input.trim();
  } catch (error) {
    return String(error);
  } finally {
    console.log("done");
  }
}

export function* counted(items: number[]): Generator<number> {
  for (const item of items) {
    if (item > 0) {
      yield item;
    }
  }
}

export const asExpression = function (value: number): number {
  return value > 0 ? value : 0 - value;
};

export const arrowed = (value: number) => (value > 0 ? value : 0 - value);

export function overlapped(value: string): string;
export function overlapped(value: number): number;
export function overlapped(value: unknown): unknown {
  return value;
}
