import type { DiffFile, DiffStatus } from "../lib/api";

interface Props {
  files: DiffFile[];
  selectedIndex: number;
  onSelect: (index: number) => void;
}

const STATUS_LABEL: Record<DiffStatus, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
};

function displayPath(file: DiffFile): string {
  if (file.status === "renamed") {
    return `${file.oldPath} → ${file.newPath}`;
  }
  return file.newPath || file.oldPath;
}

export default function DiffFileList({ files, selectedIndex, onSelect }: Props) {
  if (files.length === 0) return null;

  return (
    <ul className="diff-file-list" role="listbox" aria-label="Changed files">
      {files.map((file, i) => {
        const selected = i === selectedIndex;
        return (
          <li key={`${file.oldPath}→${file.newPath}-${i}`} role="presentation">
            <button
              type="button"
              role="option"
              aria-selected={selected}
              className={`diff-file-item diff-status-${file.status} ${selected ? "selected" : ""}`}
              onClick={() => onSelect(i)}
              title={displayPath(file)}
            >
              <span className={`diff-file-badge diff-badge-${file.status}`}>
                {STATUS_LABEL[file.status]}
              </span>
              <span className="diff-file-path">{displayPath(file)}</span>
              <span className="diff-file-stats">
                {file.additions > 0 && <span className="diff-add-count">+{file.additions}</span>}
                {file.deletions > 0 && <span className="diff-del-count">−{file.deletions}</span>}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
