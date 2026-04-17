import { useEffect, useState } from "react";
import { getGitStatus } from "../lib/git";
import type { GitStatus } from "../lib/types";

interface Options {
  /** Poll interval in ms. If omitted, the status is fetched only when `path` changes. */
  pollMs?: number;
  /** If false, the hook stays idle (returns null). Useful to gate on modal visibility. */
  enabled?: boolean;
}

/**
 * Reactive git status for a directory. Returns null while loading or when the
 * directory is not a git repo.
 */
export function useGitStatus(path: string | null | undefined, options: Options = {}): GitStatus | null {
  const { pollMs, enabled = true } = options;
  const [status, setStatus] = useState<GitStatus | null>(null);

  useEffect(() => {
    if (!enabled || !path) {
      setStatus(null);
      return;
    }

    let cancelled = false;
    const fetchStatus = () => {
      getGitStatus(path)
        .then((s) => { if (!cancelled) setStatus(s); })
        .catch(() => { if (!cancelled) setStatus(null); });
    };

    fetchStatus();

    if (!pollMs) return () => { cancelled = true; };

    const interval = setInterval(fetchStatus, pollMs);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [path, pollMs, enabled]);

  return status;
}
