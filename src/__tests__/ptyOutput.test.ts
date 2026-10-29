import { describe, it, expect, vi, beforeEach } from "vitest";

const subscribePtyOutput = vi.fn();
const writeToTerminal = vi.fn();

vi.mock("../lib/api", () => ({
  subscribePtyOutput: (...args: unknown[]) => subscribePtyOutput(...args),
}));
vi.mock("../lib/terminalPool", () => ({
  writeToTerminal: (...args: unknown[]) => writeToTerminal(...args),
}));

function frame(id: number, bytes: number[]): ArrayBuffer {
  const buf = new ArrayBuffer(4 + bytes.length);
  new DataView(buf).setUint32(0, id, true);
  new Uint8Array(buf, 4).set(bytes);
  return buf;
}

type Mod = typeof import("../lib/ptyOutput");
let mod: Mod;

beforeEach(async () => {
  vi.resetModules();
  subscribePtyOutput.mockReset();
  subscribePtyOutput.mockResolvedValue(undefined);
  writeToTerminal.mockReset();
  mod = await import("../lib/ptyOutput");
});

async function subscribedChannel() {
  await mod.ensurePtyOutputSubscribed();
  return subscribePtyOutput.mock.calls[0][0] as {
    onmessage: (m: ArrayBuffer) => void;
  };
}

describe("decodeFrame", () => {
  it("decodes id and payload", () => {
    const r = mod.decodeFrame(frame(0x01020304, [104, 105]));
    expect(r?.id).toBe(0x01020304);
    expect(Array.from(r!.data)).toEqual([104, 105]);
  });

  it("decodes typed-array view with offset", () => {
    const buf = new ArrayBuffer(8);
    const view = new Uint8Array(buf, 2, 6);
    new DataView(buf, 2).setUint32(0, 7, true);
    view.set([1, 2], 4);
    const r = mod.decodeFrame(view);
    expect(r?.id).toBe(7);
    expect(Array.from(r!.data)).toEqual([1, 2]);
  });

  it("short frame returns null", () => {
    expect(mod.decodeFrame(new ArrayBuffer(3))).toBeNull();
  });
});

describe("routing", () => {
  it("routes frames to terminal in order", async () => {
    const ch = await subscribedChannel();
    ch.onmessage(frame(1, [97]));
    ch.onmessage(frame(2, [98]));
    ch.onmessage(frame(1, [99]));
    expect(writeToTerminal.mock.calls.map(([id, d]) => [id, Array.from(d)])).toEqual([
      [1, [97]],
      [2, [98]],
      [1, [99]],
    ]);
  });

  it("empty payload is not written", async () => {
    const ch = await subscribedChannel();
    ch.onmessage(frame(1, []));
    expect(writeToTerminal).not.toHaveBeenCalled();
  });

  it("passes split multi-byte bytes unchanged", async () => {
    const ch = await subscribedChannel();
    ch.onmessage(frame(1, [0xe2, 0x94]));
    ch.onmessage(frame(1, [0x80]));
    expect(writeToTerminal.mock.calls.map(([, d]) => Array.from(d))).toEqual([
      [0xe2, 0x94],
      [0x80],
    ]);
  });
});

describe("subscription", () => {
  it("concurrent first calls share one subscription", async () => {
    let resolve!: () => void;
    subscribePtyOutput.mockReturnValue(new Promise<void>((r) => (resolve = r)));
    const a = mod.ensurePtyOutputSubscribed();
    const b = mod.ensurePtyOutputSubscribed();
    resolve();
    await Promise.all([a, b]);
    expect(subscribePtyOutput).toHaveBeenCalledTimes(1);
  });

  it("later calls reuse subscription", async () => {
    await mod.ensurePtyOutputSubscribed();
    await mod.ensurePtyOutputSubscribed();
    expect(subscribePtyOutput).toHaveBeenCalledTimes(1);
  });

  it("retries after failure", async () => {
    subscribePtyOutput.mockRejectedValueOnce(new Error("boom"));
    await expect(mod.ensurePtyOutputSubscribed()).rejects.toThrow("boom");
    await expect(mod.ensurePtyOutputSubscribed()).resolves.toBeUndefined();
    expect(subscribePtyOutput).toHaveBeenCalledTimes(2);
    expect(subscribePtyOutput.mock.calls[1][0]).not.toBe(
      subscribePtyOutput.mock.calls[0][0]
    );
  });
});
