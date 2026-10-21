export interface Config {
  readonly root: string;
}

export interface Plugin {
  readonly name: string;
  apply(config: Config): void;
}

export declare function loadConfig(path: string): Config;
