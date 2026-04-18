import { useEffect, useRef, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import "@xterm/xterm/css/xterm.css";
import { writeToSession, resizeSession, saveScrollback, getScrollback } from "../lib/api";
import { useSettingsStore } from "../store/settingsStore";
import { getThemeById, resolveSystemTheme } from "../lib/themes";
import {
  MAX_LIVE_TERMINALS,
  type TerminalInstance,
  deleteTerminal,
  findLRUVictim,
  flushPendingWrites,
  getTerminal,
  poolEntries,
  poolSize,
  setTerminal,
  touchAccessOrder,
} from "../lib/terminalPool";

export { writeToTerminal, focusTerminal } from "../lib/terminalPool";

function getTerminalTheme() {
  const attr = document.documentElement.getAttribute("data-theme");
  const themeId = attr || resolveSystemTheme();
  const def = getThemeById(themeId);
  return def?.terminal ?? getThemeById("sessonix-dark")!.terminal;
}

const SAVE_INTERVAL_MS = 30_000; // 30 seconds

interface TerminalPaneProps {
  activeSessionId: number | null;
  sessionIds: number[];
  isActiveSessionExited?: boolean;
  hidden?: boolean;
}

function evictLRU(keepId: number, containerEl?: HTMLDivElement | null) {
  while (poolSize() > MAX_LIVE_TERMINALS) {
    const victim = findLRUVictim(keepId);
    if (victim === undefined) break;

    const instance = getTerminal(victim);
    if (instance && !instance.disposed) {
      try {
        const data = instance.serializeAddon.serialize();
        saveScrollback(victim, data).catch(() => {});
      } catch { /* ok */ }
      instance.terminal.dispose();
      instance.disposed = true;
    }
    deleteTerminal(victim);

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
  hidden = false,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalFontSize = useSettingsStore((s) => s.terminalFontSize);
  const terminalFontFamily = useSettingsStore((s) => s.terminalFontFamily);

  // Live-update font settings on all active terminals
  useEffect(() => {
    for (const [, inst] of poolEntries()) {
      if (inst.disposed) continue;
      inst.terminal.options.fontSize = terminalFontSize;
      inst.terminal.options.fontFamily = terminalFontFamily;
      inst.fitAddon.fit();
    }
  }, [terminalFontSize, terminalFontFamily]);

  const getOrCreateTerminal = useCallback(
    (sessionId: number, readOnly = false): TerminalInstance => {
      const existing = getTerminal(sessionId);
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
        // Buffer until scrollback-restore runs so historical state lands
        // before whatever PTY output arrives between attach and terminal.open.
        pendingWrites: [],
      };
      setTerminal(sessionId, instance);
      return instance;
    },
    []
  );

  // `isActiveSessionExited` is read inside this effect but intentionally not in
  // deps: xterm's `disableStdin` is only applied at terminal *creation*, so a
  // status tick flipping running → exited on the currently-attached terminal
  // shouldn't remount it. The effect only fires on session switch.
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

    // Restore saved scrollback for any freshly-created terminal (first open, or
    // re-opened after LRU eviction). `pendingWrites` doubles as the "needs
    // restore" flag — set in getOrCreateTerminal, cleared by flushPendingWrites.
    // Must live in this effect (not a separate one declared earlier) so it runs
    // AFTER getOrCreateTerminal adds the instance to the pool. A prior split
    // ordered the restore effect first, which saw an empty pool on new sessions,
    // early-returned, and left PTY output stuck in pendingWrites forever.
    let cancelled = false;
    if (instance.pendingWrites) {
      const id = activeSessionId;
      getScrollback(id)
        .then((data) => {
          if (cancelled || instance.disposed) return;
          flushPendingWrites(id, data || undefined);
        })
        .catch(() => {
          if (cancelled || instance.disposed) return;
          flushPendingWrites(id);
        });
    }

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (!instance.disposed) {
          fitAddon.fit();
          terminal.focus();
        }
      });
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSessionId, getOrCreateTerminal]);

  // Theme sync: update all live terminals when theme changes (manual toggle or OS)
  useEffect(() => {
    const applyTheme = () => {
      const theme = getTerminalTheme();
      for (const [, instance] of poolEntries()) {
        if (!instance.disposed) {
          instance.terminal.options.theme = theme;
        }
      }
    };

    const mq = window.matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", applyTheme);

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
      const instance = getTerminal(activeSessionId);
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
      const instance = getTerminal(activeSessionId);
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
    for (const [id, instance] of poolEntries()) {
      if (!activeIds.has(id)) {
        try {
          const data = instance.serializeAddon.serialize();
          saveScrollback(id, data).catch(() => {});
        } catch {
          // ok
        }
        instance.terminal.dispose();
        instance.disposed = true;
        deleteTerminal(id);
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
        display: activeSessionId !== null && !hidden ? "block" : "none",
      }}
    />
  );
}
