import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  MAX_LIVE_TERMINALS,
  deleteTerminal,
  findLRUVictim,
  focusTerminal,
  getTerminal,
  poolEntries,
  poolSize,
  setTerminal,
  touchAccessOrder,
  writeToTerminal,
  type TerminalInstance,
} from "../lib/terminalPool";

function makeInstance(disposed = false): TerminalInstance {
  return {
    terminal: {
      write: vi.fn(),
      focus: vi.fn(),
      dispose: vi.fn(),
    } as unknown as TerminalInstance["terminal"],
    fitAddon: { fit: vi.fn() } as unknown as TerminalInstance["fitAddon"],
    serializeAddon: {
      serialize: vi.fn(() => ""),
    } as unknown as TerminalInstance["serializeAddon"],
    disposed,
  };
}

// Pool is module-level global; reset between tests.
beforeEach(() => {
  for (const [id] of poolEntries()) deleteTerminal(id);
});

describe("terminalPool", () => {
  describe("setTerminal / getTerminal / deleteTerminal", () => {
    it("round-trips an instance", () => {
      const inst = makeInstance();
      setTerminal(1, inst);
      expect(getTerminal(1)).toBe(inst);
    });

    it("returns undefined for missing ids", () => {
      expect(getTerminal(999)).toBeUndefined();
    });

    it("delete removes the entry", () => {
      setTerminal(1, makeInstance());
      expect(poolSize()).toBe(1);
      deleteTerminal(1);
      expect(poolSize()).toBe(0);
      expect(getTerminal(1)).toBeUndefined();
    });

    it("delete also clears accessOrder", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      touchAccessOrder(1);
      touchAccessOrder(2);
      deleteTerminal(1);
      // After deletion id=1 should not be findable as victim even if keepId allows it
      expect(findLRUVictim(999)).toBe(2);
    });
  });

  describe("touchAccessOrder / findLRUVictim", () => {
    it("returns the least-recently-touched id", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      setTerminal(3, makeInstance());
      touchAccessOrder(1);
      touchAccessOrder(2);
      touchAccessOrder(3);
      expect(findLRUVictim(3)).toBe(1);
    });

    it("skips keepId", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      touchAccessOrder(1);
      touchAccessOrder(2);
      // 1 is oldest, but if we keep 1 we should get 2
      expect(findLRUVictim(1)).toBe(2);
    });

    it("re-touching moves id to most-recent", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      setTerminal(3, makeInstance());
      touchAccessOrder(1);
      touchAccessOrder(2);
      touchAccessOrder(3);
      touchAccessOrder(1); // 1 is now most-recent
      // Victim order: 2 (oldest), 3, 1
      expect(findLRUVictim(999)).toBe(2);
    });

    it("ignores ids that were removed from the pool", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      touchAccessOrder(1);
      touchAccessOrder(2);
      // Remove id 1 via the map but keep it in access order (simulated bug)
      // deleteTerminal() does both; findLRUVictim should still skip absent ids.
      deleteTerminal(1);
      expect(findLRUVictim(999)).toBe(2);
    });

    it("returns undefined when no victims are available", () => {
      expect(findLRUVictim(999)).toBeUndefined();
      setTerminal(1, makeInstance());
      touchAccessOrder(1);
      expect(findLRUVictim(1)).toBeUndefined(); // only candidate is keepId
    });
  });

  describe("writeToTerminal", () => {
    it("writes bytes to the matching live terminal", () => {
      const inst = makeInstance();
      setTerminal(7, inst);
      const data = new Uint8Array([1, 2, 3]);
      writeToTerminal(7, data);
      expect(inst.terminal.write).toHaveBeenCalledWith(data);
    });

    it("is a no-op for disposed terminals", () => {
      const inst = makeInstance(true);
      setTerminal(7, inst);
      writeToTerminal(7, new Uint8Array([9]));
      expect(inst.terminal.write).not.toHaveBeenCalled();
    });

    it("is a no-op for missing ids", () => {
      expect(() => writeToTerminal(1234, new Uint8Array([0]))).not.toThrow();
    });
  });

  describe("focusTerminal", () => {
    it("focuses live terminals", () => {
      const inst = makeInstance();
      setTerminal(5, inst);
      focusTerminal(5);
      expect(inst.terminal.focus).toHaveBeenCalled();
    });

    it("is a no-op for disposed terminals", () => {
      const inst = makeInstance(true);
      setTerminal(5, inst);
      focusTerminal(5);
      expect(inst.terminal.focus).not.toHaveBeenCalled();
    });

    it("is a no-op for missing ids", () => {
      expect(() => focusTerminal(9999)).not.toThrow();
    });
  });

  describe("poolEntries", () => {
    it("returns a snapshot that is safe to mutate against", () => {
      setTerminal(1, makeInstance());
      setTerminal(2, makeInstance());
      // Mutate pool mid-iteration; the snapshot must still yield both.
      const seen: number[] = [];
      for (const [id] of poolEntries()) {
        seen.push(id);
        deleteTerminal(id);
      }
      expect(seen.sort()).toEqual([1, 2]);
      expect(poolSize()).toBe(0);
    });
  });

  describe("MAX_LIVE_TERMINALS", () => {
    it("is 5", () => {
      expect(MAX_LIVE_TERMINALS).toBe(5);
    });
  });
});
