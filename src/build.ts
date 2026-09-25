/** Build-time stamp injected by Vite's `define` (see vite.config.ts). */
declare const __BUILD_STAMP__: string;

export const BUILD_STAMP: string =
  typeof __BUILD_STAMP__ === "string" ? __BUILD_STAMP__ : "dev";
