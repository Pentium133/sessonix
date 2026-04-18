import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";

export interface TerminalInstance {
  terminal: Terminal;
  fitAddon: FitAddon;
  serializeAddon: SerializeAddon;
  disposed: boolean;
  /**
   * Writes queue here until scrollback restore resolves. `undefined` means
   * the terminal is "ready" — writes go directly to xterm. `[]` means still
   * loading; live PTY output must wait so the restored history stays on top.
   */
  pendingWrites?: Uint8Array[];
}

export const MAX_LIVE_TERMINALS = 5;

// Module-level state: xterm.js terminals persist across React renders/remounts.
// Keyed by session ID (PTY id). Exported as an immutable interface.
const pool = new Map<number, TerminalInstance>();
const accessOrder: number[] = [];

export function getTerminal(sessionId: number): TerminalInstance | undefined {
  return pool.get(sessionId);
}

export function setTerminal(sessionId: number, instance: TerminalInstance): void {
  pool.set(sessionId, instance);
}

export function deleteTerminal(sessionId: number): void {
  pool.delete(sessionId);
  const idx = accessOrder.indexOf(sessionId);
  if (idx !== -1) accessOrder.splice(idx, 1);
}

export function poolSize(): number {
  return pool.size;
}

/** Snapshot of current pool entries; safe to mutate the pool mid-iteration. */
export function poolEntries(): Array<[number, TerminalInstance]> {
  return Array.from(pool.entries());
}

export function touchAccessOrder(sessionId: number): void {
  const idx = accessOrder.indexOf(sessionId);
  if (idx !== -1) accessOrder.splice(idx, 1);
  accessOrder.push(sessionId);
}

/** Returns the LRU session id that is not `keepId` and is still in the pool, or undefined. */
export function findLRUVictim(keepId: number): number | undefined {
  return accessOrder.find((id) => id !== keepId && pool.has(id));
}

export function writeToTerminal(sessionId: number, data: Uint8Array): void {
  const instance = pool.get(sessionId);
  if (!instance || instance.disposed) return;
  if (instance.pendingWrites) {
    instance.pendingWrites.push(data);
    return;
  }
  instance.terminal.write(data);
}

/**
 * Flush queued writes after scrollback restore finishes. Call order inside:
 * scrollback (if any) → queued PTY output → future writes go direct. JS is
 * single-threaded so no concurrent writes can race this sequence.
 */
export function flushPendingWrites(sessionId: number, scrollback?: string): void {
  const instance = pool.get(sessionId);
  if (!instance || instance.disposed) return;
  const queue = instance.pendingWrites ?? [];
  if (scrollback) instance.terminal.write(scrollback);
  for (const chunk of queue) {
    if (instance.disposed) return;
    instance.terminal.write(chunk);
  }
  instance.pendingWrites = undefined;
}

export function focusTerminal(sessionId: number): void {
  const instance = pool.get(sessionId);
  if (instance && !instance.disposed) {
    instance.terminal.focus();
  }
}
