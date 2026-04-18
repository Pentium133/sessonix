use git2::{DiffFindOptions, DiffOptions, Repository};
use serde::Serialize;
use std::path::Path;

const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_FILE_LINES: usize = 5_000;
const MAX_FILES: usize = 500;
const RENAME_THRESHOLD: u16 = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiff {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub files: Vec<DiffFile>,
    pub truncated_files: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub old_path: String,
    pub new_path: String,
    pub status: DiffStatus,
    pub additions: u32,
    pub deletions: u32,
    pub payload: DiffPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DiffPayload {
    #[serde(rename_all = "camelCase")]
    Text {
        old_content: String,
        new_content: String,
    },
    Binary,
    #[serde(rename_all = "camelCase")]
    TooLarge {
        size_bytes: u64,
    },
}

pub fn get_worktree_diff(working_dir: &str) -> Result<WorktreeDiff, String> {
    let path = Path::new(working_dir);
    if !path.exists() || !path.is_dir() {
        return Err(format!("Working dir not found: {}", working_dir));
    }

    let repo = match Repository::discover(working_dir) {
        Ok(r) => r,
        Err(_) => {
            return Ok(WorktreeDiff {
                is_repo: false,
                branch: None,
                head_sha: None,
                files: Vec::new(),
                truncated_files: 0,
            });
        }
    };

    let head = repo.head().ok();
    let branch = head.as_ref().and_then(|h| h.shorthand()).map(String::from);
    let head_sha = head
        .as_ref()
        .and_then(|h| h.target())
        .map(|oid| format!("{:.7}", oid));

    // HEAD tree for the diff base. Missing HEAD (freshly-initialized repo
    // before any commits) — diff everything in the workdir as additions by
    // passing None as the tree.
    let head_tree = head
        .as_ref()
        .and_then(|h| h.peel_to_tree().ok());

    let mut opts = DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true)
        .include_typechange(true);

    let mut diff = repo
        .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
        .map_err(|e| format!("git2 diff failed: {}", e))?;

    let mut find_opts = DiffFindOptions::new();
    find_opts
        .renames(true)
        .rename_threshold(RENAME_THRESHOLD)
        .renames_from_rewrites(true)
        .for_untracked(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| format!("git2 find_similar failed: {}", e))?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| "Bare repository not supported".to_string())?
        .to_path_buf();

    let total_deltas = diff.deltas().len();
    let mut files: Vec<DiffFile> = Vec::new();
    let mut truncated_files: u32 = 0;

    for (idx, delta) in diff.deltas().enumerate() {
        if idx >= MAX_FILES {
            truncated_files = (total_deltas - MAX_FILES) as u32;
            break;
        }

        let (additions, deletions) = match git2::Patch::from_diff(&diff, idx) {
            Ok(Some(patch)) => patch
                .line_stats()
                .map(|(_ctx, add, del)| (add as u32, del as u32))
                .unwrap_or((0, 0)),
            _ => (0, 0),
        };

        let status = match delta.status() {
            git2::Delta::Added | git2::Delta::Untracked | git2::Delta::Copied => DiffStatus::Added,
            git2::Delta::Deleted => DiffStatus::Deleted,
            git2::Delta::Renamed => DiffStatus::Renamed,
            git2::Delta::Modified
            | git2::Delta::Typechange
            | git2::Delta::Conflicted => DiffStatus::Modified,
            _ => continue,
        };

        let old_path = delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let new_path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Normalize paths to empty string when the side doesn't exist.
        let old_path = if matches!(status, DiffStatus::Added) {
            String::new()
        } else {
            old_path
        };
        let new_path = if matches!(status, DiffStatus::Deleted) {
            String::new()
        } else {
            new_path
        };

        let is_binary = delta.old_file().is_binary()
            || delta.new_file().is_binary()
            || sniff_binary(&repo, &workdir, delta.old_file().id(), delta.old_file().path())
            || sniff_binary(&repo, &workdir, delta.new_file().id(), delta.new_file().path());
        if is_binary {
            files.push(DiffFile {
                old_path,
                new_path,
                status,
                additions,
                deletions,
                payload: DiffPayload::Binary,
            });
            continue;
        }

        // git2 reports blob size accurately for tracked files, but for untracked
        // files `size()` can be 0 since no blob exists yet — so we also probe
        // the workdir file on disk before reading it into memory.
        let old_blob_size = delta.old_file().size();
        let new_blob_size = delta.new_file().size();
        let new_fs_size = delta
            .new_file()
            .path()
            .map(|p| std::fs::metadata(workdir.join(p)).map(|m| m.len()).unwrap_or(0))
            .unwrap_or(0);
        let max_size = old_blob_size.max(new_blob_size).max(new_fs_size);
        if max_size > MAX_FILE_BYTES {
            files.push(DiffFile {
                old_path,
                new_path,
                status,
                additions,
                deletions,
                payload: DiffPayload::TooLarge { size_bytes: max_size },
            });
            continue;
        }

        let old_content = if !matches!(status, DiffStatus::Added) {
            read_blob_content(&repo, delta.old_file().id())
        } else {
            String::new()
        };

        let new_content = if !matches!(status, DiffStatus::Deleted) {
            read_workdir_or_blob(&repo, &workdir, &delta.new_file())
        } else {
            String::new()
        };

        let line_count = old_content.lines().count().max(new_content.lines().count());
        if line_count > MAX_FILE_LINES {
            files.push(DiffFile {
                old_path,
                new_path,
                status,
                additions,
                deletions,
                payload: DiffPayload::TooLarge { size_bytes: max_size },
            });
            continue;
        }

        files.push(DiffFile {
            old_path,
            new_path,
            status,
            additions,
            deletions,
            payload: DiffPayload::Text {
                old_content,
                new_content,
            },
        });
    }

    Ok(WorktreeDiff {
        is_repo: true,
        branch,
        head_sha,
        files,
        truncated_files,
    })
}

const BINARY_SNIFF_BYTES: usize = 8_000;

/// git2 doesn't always pre-flag untracked files as binary. Sniff the first 8 KB
/// for a NUL byte (same heuristic git uses).
fn sniff_binary(
    repo: &Repository,
    workdir: &Path,
    oid: git2::Oid,
    rel: Option<&Path>,
) -> bool {
    if let Some(rel) = rel {
        let abs = workdir.join(rel);
        if let Ok(mut bytes) = std::fs::read(&abs) {
            bytes.truncate(BINARY_SNIFF_BYTES);
            if bytes.contains(&0u8) {
                return true;
            }
        }
    }
    if !oid.is_zero() {
        if let Ok(blob) = repo.find_blob(oid) {
            let sample = &blob.content()[..blob.content().len().min(BINARY_SNIFF_BYTES)];
            if sample.contains(&0u8) {
                return true;
            }
        }
    }
    false
}

fn read_blob_content(repo: &Repository, oid: git2::Oid) -> String {
    if oid.is_zero() {
        return String::new();
    }
    match repo.find_blob(oid) {
        Ok(blob) => String::from_utf8_lossy(blob.content()).into_owned(),
        Err(_) => String::new(),
    }
}

/// For the working-tree side, prefer the actual on-disk file contents so that
/// unsaved edits and untracked files are visible. Fall back to the blob (when
/// the file has since been removed from the workdir but is still staged).
fn read_workdir_or_blob(repo: &Repository, workdir: &Path, entry: &git2::DiffFile) -> String {
    if let Some(rel) = entry.path() {
        let abs = workdir.join(rel);
        if let Ok(bytes) = std::fs::read(&abs) {
            return String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    read_blob_content(repo, entry.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo_with_commit(path: &Path) -> Repository {
        let repo = Repository::init(path).unwrap();
        {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }
        repo
    }

    fn commit_file(repo: &Repository, rel: &str, contents: &[u8]) {
        let workdir = repo.workdir().unwrap().to_path_buf();
        if let Some(parent) = workdir.join(rel).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(workdir.join(rel), contents).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(rel)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("add {}", rel),
            &tree,
            &[&parent],
        )
        .unwrap();
    }

    #[test]
    fn test_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert!(!diff.is_repo);
        assert!(diff.files.is_empty());
        assert_eq!(diff.truncated_files, 0);
    }

    #[test]
    fn test_invalid_working_dir() {
        let err = get_worktree_diff("/definitely/not/a/real/path/xyz123").unwrap_err();
        assert!(err.contains("Working dir not found"));
    }

    #[test]
    fn test_empty_diff() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert!(diff.is_repo);
        assert!(diff.files.is_empty());
        assert_eq!(diff.truncated_files, 0);
        assert!(diff.head_sha.is_some());
    }

    #[test]
    fn test_modified_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        commit_file(&repo, "hello.txt", b"old contents\n");
        fs::write(tmp.path().join("hello.txt"), b"new contents\n").unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.status, DiffStatus::Modified);
        assert_eq!(f.old_path, "hello.txt");
        assert_eq!(f.new_path, "hello.txt");
        assert_eq!(f.additions, 1);
        assert_eq!(f.deletions, 1);
        match &f.payload {
            DiffPayload::Text { old_content, new_content } => {
                assert_eq!(old_content, "old contents\n");
                assert_eq!(new_content, "new contents\n");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_added_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        fs::write(tmp.path().join("fresh.txt"), b"brand new\n").unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.status, DiffStatus::Added);
        assert_eq!(f.old_path, "");
        assert_eq!(f.new_path, "fresh.txt");
        match &f.payload {
            DiffPayload::Text { old_content, new_content } => {
                assert_eq!(old_content, "");
                assert_eq!(new_content, "brand new\n");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_deleted_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        commit_file(&repo, "gone.txt", b"bye\n");
        fs::remove_file(tmp.path().join("gone.txt")).unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.status, DiffStatus::Deleted);
        assert_eq!(f.old_path, "gone.txt");
        assert_eq!(f.new_path, "");
        match &f.payload {
            DiffPayload::Text { old_content, new_content } => {
                assert_eq!(old_content, "bye\n");
                assert_eq!(new_content, "");
            }
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_rename_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        // Long content so ≥50% similarity is easy to hit after a rename+tiny edit.
        let body = "line-a\nline-b\nline-c\nline-d\nline-e\nline-f\nline-g\nline-h\n";
        commit_file(&repo, "original.txt", body.as_bytes());

        fs::remove_file(tmp.path().join("original.txt")).unwrap();
        let mut new_body = body.to_string();
        new_body.push_str("line-i\n"); // tiny edit, still similar
        fs::write(tmp.path().join("renamed.txt"), new_body.as_bytes()).unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1, "expected one Renamed entry, got {:?}", diff.files);
        let f = &diff.files[0];
        assert_eq!(f.status, DiffStatus::Renamed);
        assert_eq!(f.old_path, "original.txt");
        assert_eq!(f.new_path, "renamed.txt");
    }

    #[test]
    fn test_rename_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        commit_file(&repo, "one.txt", b"aaa\n");
        fs::remove_file(tmp.path().join("one.txt")).unwrap();
        fs::write(tmp.path().join("two.txt"), b"completely different\n").unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        // Should have Deleted + Added, not a single Renamed.
        assert!(diff.files.iter().any(|f| f.status == DiffStatus::Deleted));
        assert!(diff.files.iter().any(|f| f.status == DiffStatus::Added));
        assert!(!diff.files.iter().any(|f| f.status == DiffStatus::Renamed));
    }

    #[test]
    fn test_binary_file() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        // Enough NUL bytes that git2 classifies it as binary.
        let mut bytes = vec![0u8; 2048];
        bytes[0..4].copy_from_slice(&[0x89, b'P', b'N', b'G']);
        fs::write(tmp.path().join("img.bin"), &bytes).unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        let f = &diff.files[0];
        assert_eq!(f.status, DiffStatus::Added);
        assert!(matches!(f.payload, DiffPayload::Binary));
    }

    #[test]
    fn test_large_file() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        // 1.5 MB text file, exceeds MAX_FILE_BYTES.
        let big = "line\n".repeat(300_000);
        fs::write(tmp.path().join("big.txt"), &big).unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        match &diff.files[0].payload {
            DiffPayload::TooLarge { size_bytes } => {
                assert!(*size_bytes > MAX_FILE_BYTES);
            }
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }

    #[test]
    fn test_large_untracked_file_stub_uses_fs_metadata() {
        // Regression: previously the untracked path was classified by git2's
        // `DiffFile::size()`, which can be 0 for untracked entries — that let
        // multi-megabyte ASCII logs slip past the byte cap and get read fully
        // into memory before the line-count filter caught them. Now we probe
        // `fs::metadata` on the workdir path as a fallback.
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        // 2 MB of a single line to avoid tripping the line-count limit.
        let payload = "x".repeat(2 * 1024 * 1024);
        fs::write(tmp.path().join("huge.log"), &payload).unwrap();

        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), 1);
        match &diff.files[0].payload {
            DiffPayload::TooLarge { size_bytes } => {
                assert!(*size_bytes > MAX_FILE_BYTES);
            }
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }

    #[test]
    fn test_500_file_limit() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        for i in 0..MAX_FILES + 10 {
            fs::write(tmp.path().join(format!("f{:04}.txt", i)), b"x\n").unwrap();
        }
        let diff = get_worktree_diff(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(diff.files.len(), MAX_FILES);
        assert_eq!(diff.truncated_files, 10);
    }
}
