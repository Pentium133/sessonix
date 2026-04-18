import { useMemo, useState } from "react";
import { useProjectStore } from "../store/projectStore";
import { useSessionStore } from "../store/sessionStore";
import { useWorktreeDiff } from "../hooks/useWorktreeDiff";
import DiffFileList from "./DiffFileList";
import DiffFilePane from "./DiffFilePane";

export default function DiffViewer() {
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);
  const lastActiveSession = useProjectStore((s) => s.lastActiveSession);
  const sessions = useSessionStore((s) => s.sessions);

  const workingDir = useMemo(() => {
    if (!activeProjectPath) return null;
    const lastId = lastActiveSession[activeProjectPath];
    if (lastId) {
      const s = sessions.find((x) => x.id === lastId);
      if (s) return s.worktree_path || s.working_dir;
    }
    return activeProjectPath;
  }, [activeProjectPath, lastActiveSession, sessions]);

  const { data, loading, error, refresh } = useWorktreeDiff(workingDir);

  // `selectedIndex = -1` means "let the list auto-pick the first entry".
  const [selectedIndex, setSelectedIndex] = useState<number>(-1);

  if (!activeProjectPath) {
    return (
      <div className="diff-viewer diff-state">
        <p>Select a project to view its diff.</p>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="diff-viewer diff-state">
        <div className="diff-skeleton">
          <div className="diff-skeleton-list" />
          <div className="diff-skeleton-pane" />
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="diff-viewer diff-state">
        <p className="diff-error">Error: {error}</p>
        <button type="button" className="diff-refresh-btn" onClick={refresh}>
          Retry
        </button>
      </div>
    );
  }

  if (!data) {
    return <div className="diff-viewer diff-state" />;
  }

  if (!data.isRepo) {
    return (
      <div className="diff-viewer diff-state">
        <p>Not a git repository.</p>
      </div>
    );
  }

  if (data.files.length === 0) {
    return (
      <div className="diff-viewer diff-state">
        <p className="diff-empty-heading">No changes</p>
        <p className="diff-empty-sub">
          {data.branch ?? "detached"}
          {data.headSha ? ` · ${data.headSha}` : ""}
        </p>
        <button type="button" className="diff-refresh-btn" onClick={refresh}>
          Refresh
        </button>
      </div>
    );
  }

  const effectiveIndex =
    selectedIndex >= 0 && selectedIndex < data.files.length ? selectedIndex : 0;
  const selectedFile = data.files[effectiveIndex] ?? null;

  return (
    <div className="diff-viewer">
      <aside className="diff-viewer-sidebar">
        <header className="diff-viewer-header">
          <span className="diff-viewer-branch">
            {data.branch ?? "detached"}
            {data.headSha ? ` · ${data.headSha}` : ""}
          </span>
          <button type="button" className="diff-refresh-btn" onClick={refresh} aria-label="Refresh diff">
            Refresh
          </button>
        </header>
        {data.truncatedFiles > 0 && (
          <div className="diff-truncation-banner" role="status">
            {data.truncatedFiles} more files hidden — reduce the scope of your changes
          </div>
        )}
        <DiffFileList
          files={data.files}
          selectedIndex={effectiveIndex}
          onSelect={setSelectedIndex}
        />
      </aside>
      <main className="diff-viewer-main">
        <DiffFilePane file={selectedFile} />
      </main>
    </div>
  );
}
