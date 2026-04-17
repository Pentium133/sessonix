import { useEffect, useRef } from "react";

/**
 * Run `handler` when the user presses Escape. Listener is attached only while
 * `enabled` is true; callers don't need to memoize `handler` — the latest
 * reference is captured in a ref so the listener binds exactly once per
 * `enabled` transition.
 */
export function useEscapeKey(handler: () => void, enabled = true) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") handlerRef.current();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [enabled]);
}
