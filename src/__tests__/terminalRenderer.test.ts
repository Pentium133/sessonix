import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { Terminal } from "@xterm/xterm";

const webglState = vi.hoisted(() => ({
  throwOnConstruct: false,
  instances: [] as Array<{ dispose: ReturnType<typeof vi.fn>; contextLoss?: () => void }>,
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: vi.fn(function (this: Record<string, unknown>) {
    if (webglState.throwOnConstruct) throw new Error("no webgl2");
    const inst = {
      dispose: vi.fn(),
      onContextLoss: (cb: () => void) => {
        inst.contextLoss = cb;
      },
    } as { dispose: ReturnType<typeof vi.fn>; contextLoss?: () => void; onContextLoss: (cb: () => void) => void };
    webglState.instances.push(inst);
    return inst;
  }),
}));

vi.mock("@xterm/addon-unicode11", () => ({
  Unicode11Addon: vi.fn(function () {
    return { kind: "unicode11" };
  }),
}));

import {
  loadUnicodeWidths,
  loadWebglRenderer,
  onFontsLoaded,
  preloadTerminalFonts,
} from "../lib/terminalRenderer";

function stubTerminal() {
  return {
    loadAddon: vi.fn(),
    unicode: { activeVersion: "6" },
  } as unknown as Terminal & { loadAddon: ReturnType<typeof vi.fn> };
}

describe("terminalRenderer", () => {
  beforeEach(() => {
    webglState.throwOnConstruct = false;
    webglState.instances = [];
    vi.restoreAllMocks();
  });

  it("loadUnicodeWidths loads addon and activates v11", () => {
    const terminal = stubTerminal();
    loadUnicodeWidths(terminal);
    expect(terminal.loadAddon).toHaveBeenCalledTimes(1);
    expect(terminal.loadAddon.mock.calls[0][0]).toEqual({ kind: "unicode11" });
    expect(terminal.unicode.activeVersion).toBe("11");
  });

  it("loadWebglRenderer loads webgl addon", () => {
    const terminal = stubTerminal();
    loadWebglRenderer(terminal);
    expect(terminal.loadAddon).toHaveBeenCalledWith(webglState.instances[0]);
  });

  it("webgl constructor failure falls back", () => {
    webglState.throwOnConstruct = true;
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const terminal = stubTerminal();
    expect(() => loadWebglRenderer(terminal)).not.toThrow();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(terminal.loadAddon).not.toHaveBeenCalled();
  });

  it("context loss disposes addon", () => {
    const terminal = stubTerminal();
    loadWebglRenderer(terminal);
    const inst = webglState.instances[0];
    inst.contextLoss!();
    expect(inst.dispose).toHaveBeenCalledTimes(1);
  });
});

describe("terminal fonts", () => {
  let fonts: EventTarget & { load: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    fonts = Object.assign(new EventTarget(), {
      load: vi.fn(() => Promise.resolve([])),
    });
    Object.defineProperty(document, "fonts", { value: fonts, configurable: true });
  });

  afterEach(() => {
    Reflect.deleteProperty(document, "fonts");
  });

  it("preloads regular and bold faces of the configured font", () => {
    preloadTerminalFonts("'JetBrains Mono', monospace", 13);
    expect(fonts.load).toHaveBeenCalledWith("normal 13px 'JetBrains Mono', monospace");
    expect(fonts.load).toHaveBeenCalledWith("bold 13px 'JetBrains Mono', monospace");
  });

  it("swallows rejected and throwing loads", async () => {
    fonts.load.mockImplementationOnce(() => Promise.reject(new Error("network")));
    fonts.load.mockImplementationOnce(() => {
      throw new SyntaxError("bad font");
    });
    expect(() => preloadTerminalFonts("x", 13)).not.toThrow();
    await Promise.resolve();
  });

  it("is a no-op without document.fonts", () => {
    Reflect.deleteProperty(document, "fonts");
    expect(() => preloadTerminalFonts("x", 13)).not.toThrow();
    const off = onFontsLoaded(vi.fn());
    expect(() => off()).not.toThrow();
  });

  it("notifies on every loadingdone until unsubscribed", () => {
    const cb = vi.fn();
    const off = onFontsLoaded(cb);
    fonts.dispatchEvent(new Event("loadingdone"));
    fonts.dispatchEvent(new Event("loadingdone"));
    expect(cb).toHaveBeenCalledTimes(2);
    off();
    fonts.dispatchEvent(new Event("loadingdone"));
    expect(cb).toHaveBeenCalledTimes(2);
  });
});
