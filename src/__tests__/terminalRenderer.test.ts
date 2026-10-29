import { describe, it, expect, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";

vi.mock("@xterm/addon-unicode11", () => ({
  Unicode11Addon: vi.fn(function () {
    return { kind: "unicode11" };
  }),
}));

import { loadUnicodeWidths } from "../lib/terminalRenderer";

function stubTerminal() {
  return {
    loadAddon: vi.fn(),
    unicode: { activeVersion: "6" },
  } as unknown as Terminal & { loadAddon: ReturnType<typeof vi.fn> };
}

describe("terminalRenderer", () => {
  it("loadUnicodeWidths loads addon and activates v11", () => {
    const terminal = stubTerminal();
    loadUnicodeWidths(terminal);
    expect(terminal.loadAddon).toHaveBeenCalledTimes(1);
    expect(terminal.loadAddon.mock.calls[0][0]).toEqual({ kind: "unicode11" });
    expect(terminal.unicode.activeVersion).toBe("11");
  });
});
