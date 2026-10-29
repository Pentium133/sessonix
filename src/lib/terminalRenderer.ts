import type { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import { Unicode11Addon } from "@xterm/addon-unicode11";

/**
 * Unicode 11 width tables so emoji and box-drawing in agent TUIs occupy the
 * right number of cells. Call before anything is written to the terminal.
 */
export function loadUnicodeWidths(terminal: Terminal) {
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";
}

/**
 * GPU renderer; must be loaded after terminal.open(). On failure or GL context
 * loss xterm falls back to its DOM renderer once the addon is disposed.
 */
export function loadWebglRenderer(terminal: Terminal) {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    terminal.loadAddon(webgl);
  } catch (e) {
    console.warn("WebGL renderer unavailable, using DOM renderer", e);
  }
}

// Web fonts are split into unicode-range subsets that load only when text in
// their range is requested; canvas drawing doesn't request them.
const PRELOAD_SAMPLE = "Aa Бб Ёё Ωω";

/**
 * Start loading the regular and bold faces of `fontFamily` now. Faces load
 * lazily, and a glyph the WebGL renderer draws before its face arrives is
 * cached in the atlas with a fallback font.
 */
export function preloadTerminalFonts(fontFamily: string, fontSize: number) {
  const fonts = document.fonts;
  if (!fonts) return;
  for (const weight of ["normal", "bold"]) {
    try {
      fonts.load(`${weight} ${fontSize}px ${fontFamily}`, PRELOAD_SAMPLE).catch(() => {});
    } catch {
      // Invalid font shorthand — nothing to preload.
    }
  }
}

/**
 * Call `onLoaded` each time the document finishes loading fonts, so terminals
 * can drop atlas glyphs rasterized with a fallback. Returns an unsubscribe.
 */
export function onFontsLoaded(onLoaded: () => void): () => void {
  const fonts = document.fonts;
  if (!fonts) return () => {};
  fonts.addEventListener("loadingdone", onLoaded);
  return () => fonts.removeEventListener("loadingdone", onLoaded);
}
