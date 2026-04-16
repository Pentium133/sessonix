import { useEffect, useRef, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import "@xterm/xterm/css/xterm.css";
import { writeToSession, resizeSession, saveScrollback, getScrollback } from "../lib/api";
import { useSettingsStore } from "../store/settingsStore";
import { getThemeById, resolveSystemTheme } from "../lib/themes";

interface TerminalInstance {
  terminal: Terminal;
  fitAddon: FitAddon;
  serializeAddon: SerializeAddon;
  disposed: boolean;
}

function getTerminalTheme() {
  const attr = document.documentElement.getAttribute("data-theme");
  const themeId = attr || resolveSystemTheme();
  const def = getThemeById(themeId);
  return def?.terminal ?? getThemeById("sessonix-dark")!.terminal;
}

const SAVE_INTERVAL_MS = 30_000; // 30 seconds
const MAX_LIVE_TERMINALS = 5;

// Track access order for LRU eviction
const accessOrder: number[] = [];

interface TerminalPaneProps {
  activeSessionId: number | null;
  sessionIds: number[];
  isActiveSessionExited?: boolean;
}

// Persistent terminal instances keyed by session ID (PTY id)
const terminalPool = new Map<number, TerminalInstance>();

function touchAccessOrder(sessionId: number) {
  const idx = accessOrder.indexOf(sessionId);
  if (idx !== -1) accessOrder.splice(idx, 1);
  accessOrder.push(sessionId);
}

function evictLRU(keepId: number, containerEl?: HTMLDivElement | null) {
  while (terminalPool.size > MAX_LIVE_TERMINALS) {
    // Find the least recently used terminal (not the one we're about to show)
    const victim = accessOrder.find((id) => id !== keepId && terminalPool.has(id));
    if (victim === undefined) break;

    const instance = terminalPool.get(victim);
    if (instance && !instance.disposed) {
      // Save scrollback before evicting
      try {
        const data = instance.serializeAddon.serialize();
        saveScrollback(victim, data).catch(() => {});
      } catch { /* ok */ }
      instance.terminal.dispose();
      instance.disposed = true;
    }
    terminalPool.delete(victim);
    const idx = accessOrder.indexOf(victim);
    if (idx !== -1) accessOrder.splice(idx, 1);

    // Remove DOM wrapper
    if (containerEl) {
      const wrapper = containerEl.querySelector(`[data-session-id="${victim}"]`);
      wrapper?.remove();
    }
  }
}

export default function TerminalPane({
  activeSessionId,
  sessionIds,
  isActiveSessionExited,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalFontSize = useSettingsStore((s) => s.terminalFontSize);
  const terminalFontFamily = useSettingsStore((s) => s.terminalFontFamily);

  // Live-update font settings on all active terminals
  useEffect(() => {
    for (const [, inst] of terminalPool) {
      if (inst.disposed) continue;
      inst.terminal.options.fontSize = terminalFontSize;
      inst.terminal.options.fontFamily = terminalFontFamily;
      inst.fitAddon.fit();
    }
  }, [terminalFontSize, terminalFontFamily]);

  const getOrCreateTerminal = useCallback(
    (sessionId: number, readOnly = false): TerminalInstance => {
      const existing = terminalPool.get(sessionId);
      if (existing && !existing.disposed) return existing;

      const settings = useSettingsStore.getState();
      const terminal = new Terminal({
        theme: getTerminalTheme(),
        fontFamily: settings.terminalFontFamily,
        fontSize: settings.terminalFontSize,
        lineHeight: 1.2,
        cursorBlink: !readOnly,
        cursorStyle: "bar",
        scrollback: 10000,
        allowProposedApi: true,
        disableStdin: readOnly,
      });

      const fitAddon = new FitAddon();
      const serializeAddon = new SerializeAddon();
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(serializeAddon);

      if (!readOnly) {
        terminal.onData((data) => {
          const bytes = Array.from(new TextEncoder().encode(data));
          writeToSession(sessionId, bytes).catch(console.error);
        });

        terminal.onResize(({ cols, rows }) => {
          resizeSession(sessionId, cols, rows).catch(console.error);
        });
      }

      const instance: TerminalInstance = {
        terminal,
        fitAddon,
        serializeAddon,
        disposed: false,
      };
      terminalPool.set(sessionId, instance);
      return instance;
    },
    []
  );

  // Restore scrollback for exited sessions
  useEffect(() => {
    if (activeSessionId === null || !isActiveSessionExited) return;

    // Only restore once per session
    const instance = terminalPool.get(activeSessionId);
    if (!instance || instance.disposed) return;
    if ((instance as TerminalInstance & { _scrollbackRestored?: boolean })._scrollbackRestored) return;

    getScrollback(activeSessionId).then((data) => {
      if (data) {
        instance.terminal.write(data);
        (instance as TerminalInstance & { _scrollbackRestored?: boolean })._scrollbackRestored = true;
      }
    }).catch(console.error);
  }, [activeSessionId, isActiveSessionExited]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || activeSessionId === null) return;

    const instance = getOrCreateTerminal(activeSessionId, isActiveSessionExited);
    const { terminal, fitAddon } = instance;

    touchAccessOrder(activeSessionId);
    evictLRU(activeSessionId, container);

    let wrapper = container.querySelector(
      `[data-session-id="${activeSessionId}"]`
    ) as HTMLDivElement | null;

    if (!wrapper) {
      wrapper = document.createElement("div");
      wrapper.dataset.sessionId = String(activeSessionId);
      wrapper.style.width = "100%";
      wrapper.style.height = "100%";
      wrapper.style.position = "absolute";
      wrapper.style.top = "0";
      wrapper.style.left = "0";
      container.appendChild(wrapper);
      terminal.open(wrapper);
    }

    for (const child of Array.from(container.children) as HTMLDivElement[]) {
      child.style.display =
        child.dataset.sessionId === String(activeSessionId) ? "block" : "none";
    }

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (!instance.disposed) {
          fitAddon.fit();
          terminal.focus();
        }
      });
    });
  }, [activeSessionId, getOrCreateTerminal]);

  // Theme sync: update all live terminals when theme changes (manual toggle or OS)
  useEffect(() => {
    const applyTheme = () => {
      const theme = getTerminalTheme();
      for (const [, instance] of terminalPool) {
        if (!instance.disposed) {
          instance.terminal.options.theme = theme;
        }
      }
    };

    // Listen for OS theme changes
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", applyTheme);

    // Listen for manual data-theme attribute changes
    const observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.attributeName === "data-theme") {
          applyTheme();
          break;
        }
      }
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    return () => {
      mq.removeEventListener("change", applyTheme);
      observer.disconnect();
    };
  }, []);

  // ResizeObserver
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const observer = new ResizeObserver(() => {
      if (activeSessionId === null) return;
      const instance = terminalPool.get(activeSessionId);
      if (instance && !instance.disposed) {
        requestAnimationFrame(() => {
          instance.fitAddon.fit();
        });
      }
    });

    observer.observe(container);
    return () => observer.disconnect();
  }, [activeSessionId]);

  // Periodic scrollback save (every 30s for active terminal only)
  useEffect(() => {
    const interval = setInterval(() => {
      if (activeSessionId === null) return;
      const instance = terminalPool.get(activeSessionId);
      if (!instance || instance.disposed) return;
      try {
        const data = instance.serializeAddon.serialize();
        saveScrollback(activeSessionId, data).catch(() => {});
      } catch {
        // serialize can fail if terminal not yet opened
      }
    }, SAVE_INTERVAL_MS);

    return () => clearInterval(interval);
  }, [activeSessionId]);

  // Cleanup destroyed sessions (save scrollback before disposing)
  useEffect(() => {
    const activeIds = new Set(sessionIds);
    for (const [id, instance] of terminalPool) {
      if (!activeIds.has(id)) {
        // Save final scrollback
        try {
          const data = instance.serializeAddon.serialize();
          saveScrollback(id, data).catch(() => {});
        } catch {
          // ok
        }
        instance.terminal.dispose();
        instance.disposed = true;
        terminalPool.delete(id);
        // Clean up access order to prevent memory leak
        const aoIdx = accessOrder.indexOf(id);
        if (aoIdx !== -1) accessOrder.splice(aoIdx, 1);
        const container = containerRef.current;
        if (container) {
          const wrapper = container.querySelector(
            `[data-session-id="${id}"]`
          );
          wrapper?.remove();
        }
      }
    }
  }, [sessionIds]);

  return (
    <div
      ref={containerRef}
      className="terminal-pane"
      style={{
        flex: 1,
        position: "relative",
        overflow: "hidden",
        display: activeSessionId !== null ? "block" : "none",
      }}
    />
  );
}

// Write binary data to a terminal
export function writeToTerminal(sessionId: number, data: Uint8Array) {
  const instance = terminalPool.get(sessionId);
  if (instance && !instance.disposed) {
    instance.terminal.write(data);
  }
}

