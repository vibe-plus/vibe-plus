declare module "vitest" {
  export const describe: (name: string, fn: () => void) => void;
  export const it: (name: string, fn: () => void | Promise<void>) => void;
  export const expect: (actual: unknown) => {
    toEqual(expected: unknown): void;
    toBe(expected: unknown): void;
    toContain(expected: unknown): void;
  };
}
