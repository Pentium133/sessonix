import ReactDiffViewer, { DiffMethod } from "react-diff-viewer-continued";
import type { DiffFile } from "../lib/api";

interface Props {
  file: DiffFile | null;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function DiffFilePane({ file }: Props) {
  if (!file) {
    return (
      <div className="diff-pane-empty" role="status">
        Select a file to view its diff.
      </div>
    );
  }

  if (file.payload.kind === "binary") {
    return (
      <div className="diff-pane-stub" role="status">
        Binary file — contents not shown.
      </div>
    );
  }

  if (file.payload.kind === "tooLarge") {
    return (
      <div className="diff-pane-stub" role="status">
        File too large ({formatSize(file.payload.sizeBytes)}) — not displayed.
      </div>
    );
  }

  const { oldContent, newContent } = file.payload;

  return (
    <div className="diff-pane-viewer">
      <ReactDiffViewer
        oldValue={oldContent}
        newValue={newContent}
        splitView={true}
        compareMethod={DiffMethod.LINES}
        useDarkTheme={true}
        hideLineNumbers={false}
      />
    </div>
  );
}
