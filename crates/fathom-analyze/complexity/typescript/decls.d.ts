// A `.d.ts` is the only declaration of these symbols (s1's corpus says so, and ADR-007 §4.3 keeps
// that reading). Each one is a unit with no body here, so each is `?`, and none of them is `0`.

export declare function read(path: string): string;

export declare function write(path: string, body: string): boolean;
