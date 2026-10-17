export type Handler<T = unknown> = (input: T) => void;

export default function create(kind: string): Handler<string> {
  return (input: string) => console.log(kind, input);
}

export function createAsync(kind: string): Promise<Handler<string>> {
  return Promise.resolve(create(kind));
}

export const twice = (x: number): number => x * 2;

export let counter = 0;

export function* tokens(input: string): Generator<string> {
  for (const part of input.split(' ')) yield part;
}

export async function load(url: string): Promise<string> {
  return url;
}
