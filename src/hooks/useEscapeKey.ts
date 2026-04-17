import { useEffect } from "react";

/**
 * Run `handler` when the user presses Escape. Listener is attached only while
 * `enabled` is true, so callers can pass `isOpen` without manual guards.
 */
export function useEscapeKey(handler: () => void, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") handler();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handler, enabled]);
}
