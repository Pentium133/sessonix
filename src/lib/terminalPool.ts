import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";

export interface TerminalInstance {
  terminal: Terminal;
  fitAddon: FitAddon;
  serializeAddon: SerializeAddon;
  disposed: boolean;
  scrollbackRestored?: boolean;
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

export function poolEntries(): IterableIterator<[number, TerminalInstance]> {
  return pool.entries();
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
  if (instance && !instance.disposed) {
    instance.terminal.write(data);
  }
}

export function focusTerminal(sessionId: number): void {
  const instance = pool.get(sessionId);
  if (instance && !instance.disposed) {
    instance.terminal.focus();
  }
}
