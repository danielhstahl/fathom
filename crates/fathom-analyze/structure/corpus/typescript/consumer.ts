import { Circle, area, type Shape } from './index';
import { make } from './index';
import { Point } from './geo';
import type { Config } from './types';
import { Legacy } from './legacy';
import * as Shapes from './shapes';

export function use(): number {
  const shape: Shape = new Circle(2);
  const handler = make('demo');
  handler(new Legacy.Old().run() === undefined ? Shapes.PI.toFixed(0) : 'x');
  return area(shape) + new Point(1, 1).norm();
}

export const reporter = (config: Config): string => config.extra ?? '';

export default use;
