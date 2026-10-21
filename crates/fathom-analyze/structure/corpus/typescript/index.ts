// The package barrel: everything a consumer imports, and nothing it declares.

export * from './shapes';
export { Circle, type Shape } from './shapes';
export { default as make } from './factory';
export { Point } from './geo';
export const VERSION = '1.2.3';
