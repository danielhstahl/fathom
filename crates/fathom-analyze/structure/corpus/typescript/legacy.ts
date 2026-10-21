import { Shape } from './shapes';

export function sealed(target: Function): void {
  return;
}

export function log(label: string): MethodDecorator {
  return () => {};
}

export namespace Legacy {
  export const tag = 'legacy';

  export class Old {
    @log('old')
    run(): void {
      return;
    }
  }
}

export interface Config {
  extra?: string;
}

export interface Config {
  debug?: boolean;
}

class LocalOnly {
  shape(): Shape | null {
    return null;
  }
}

export { LocalOnly as ExportedAlias };

declare module 'virtual:thing' {
  export const thing: string;
}
