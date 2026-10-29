import type { Terminal } from "@xterm/xterm";
import { Unicode11Addon } from "@xterm/addon-unicode11";

/**
 * Unicode 11 width tables so emoji and box-drawing in agent TUIs occupy the
 * right number of cells. Call before anything is written to the terminal.
 */
export function loadUnicodeWidths(terminal: Terminal) {
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";
}
