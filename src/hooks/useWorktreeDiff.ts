import { useCallback, useEffect, useRef, useState } from "react";
import { getWorktreeDiff, type WorktreeDiff } from "../lib/api";

const LOADING_DELAY_MS = 500;

interface UseWorktreeDiffResult {
  data: WorktreeDiff | null;
  loading: boolean;
  error: string | null;
  refresh: () => void;
}

export function useWorktreeDiff(workingDir: string | null): UseWorktreeDiffResult {
  const [data, setData] = useState<WorktreeDiff | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);

  const activeFetchId = useRef(0);

  const run = useCallback((dir: string) => {
    const fetchId = ++activeFetchId.current;
    setError(null);
    setLoading(false);

    const loadingTimer = window.setTimeout(() => {
      if (activeFetchId.current === fetchId) setLoading(true);
    }, LOADING_DELAY_MS);

    getWorktreeDiff(dir)
      .then((diff) => {
        if (activeFetchId.current !== fetchId) return;
        setData(diff);
        setError(null);
      })
      .catch((e: unknown) => {
        if (activeFetchId.current !== fetchId) return;
        setError(e instanceof Error ? e.message : String(e));
        setData(null);
      })
      .finally(() => {
        if (activeFetchId.current !== fetchId) return;
        window.clearTimeout(loadingTimer);
        setLoading(false);
      });

    return () => {
      window.clearTimeout(loadingTimer);
    };
  }, []);

  useEffect(() => {
    if (!workingDir) {
      setData(null);
      setError(null);
      setLoading(false);
      return;
    }
    const cleanup = run(workingDir);
    return cleanup;
    // `generation` bump re-runs the fetch on manual refresh.
  }, [workingDir, generation, run]);

  const refresh = useCallback(() => {
    setGeneration((g) => g + 1);
  }, []);

  return { data, loading, error, refresh };
}
