// The agent's tested contrast maths, declared for TypeScript. One implementation, two consumers:
// the rendered sweep on a device (agent/scripts/panel-render-audit.mjs, which embeds PROBE_SOURCE)
// and the static pair sweep here (lib/pairContrast.test.ts). Declaring the module is how the panel
// imports .mjs without a second copy of the maths drifting from the first.
declare module "*/contrast-probe.mjs" {
  export interface Rgb {
    r: number;
    g: number;
    b: number;
    a?: number;
  }
  export function parseColour(value: string): Rgb | null;
  export function contrastRatio(fg: Rgb, bg: Rgb): number;
  export function aaThreshold(fontSizePx: number, fontWeight: number | string): number;
  export function compositeStack(stack: Rgb[], base?: Rgb): Rgb;
}
