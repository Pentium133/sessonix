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
