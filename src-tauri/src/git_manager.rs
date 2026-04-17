use git2::Repository;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize)]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub changed_files: u32,
    pub modified: u32,
    pub added: u32,
    pub deleted: u32,
    pub head_sha: Option<String>,
    pub is_worktree: bool,
}

/// Get git status for a working directory.
/// Returns a default (is_repo=false) if the directory is not inside a git repo.
pub fn get_git_status(working_dir: &str) -> GitStatus {
    let repo = match Repository::discover(working_dir) {
        Ok(r) => r,
        Err(_) => return GitStatus::default(),
    };

    let head = repo.head().ok();
    let branch = head.as_ref().and_then(|h| h.shorthand()).map(String::from);
    let head_sha = head
        .as_ref()
        .and_then(|h| h.target())
        .map(|oid| format!("{:.7}", oid));

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(false)
        .exclude_submodules(true)
        .include_ignored(false);

    let (mut changed_files, mut modified, mut added, mut deleted) = (0u32, 0u32, 0u32, 0u32);
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        changed_files = statuses.len() as u32;
        for entry in statuses.iter() {
            let s = entry.status();
            if s.intersects(git2::Status::INDEX_MODIFIED | git2::Status::WT_MODIFIED | git2::Status::INDEX_RENAMED | git2::Status::WT_RENAMED) {
                modified += 1;
            } else if s.intersects(git2::Status::INDEX_NEW | git2::Status::WT_NEW) {
                added += 1;
            } else if s.intersects(git2::Status::INDEX_DELETED | git2::Status::WT_DELETED) {
                deleted += 1;
            }
        }
    }

    let is_worktree = repo.is_worktree();

    GitStatus {
        is_repo: true,
        branch,
        changed_files,
        modified,
        added,
        deleted,
        head_sha,
        is_worktree,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    pub name: String,
    /// Absolute path of the worktree where this branch is currently checked out.
    /// `None` means the branch has no active checkout and is available for a new worktree.
    pub worktree_path: Option<String>,
    /// `true` when this branch is the HEAD of the project's main workdir (i.e. the
    /// branch you'd see in the project root, whatever its name). Git refuses to
    /// attach another worktree to it, so it can't back a Task.
    pub is_project_head: bool,
}

/// List local branches in the repo containing `working_dir`, annotated with where
/// (if anywhere) each branch is currently checked out. `working_dir` may be the
/// main workdir or any linked worktree — we always operate on the shared repo.
pub fn list_branches(working_dir: &str) -> Result<Vec<BranchInfo>, String> {
    let repo = Repository::discover(working_dir).map_err(|e| format!("Not a git repo: {}", e))?;

    // When `working_dir` is itself a linked worktree we have to re-open the
    // main repo to read its HEAD and enumerate branches from the shared ref
    // store. When it is already the main workdir — the common case — reuse
    // the handle we just opened instead of paying for a second discover.
    let (main_repo, main_workdir) = if repo.is_worktree() {
        let main_workdir = repo
            .path()
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or("Cannot resolve main workdir from linked worktree")?
            .to_path_buf();
        let main_repo = Repository::open(&main_workdir)
            .map_err(|e| format!("Failed to open main repo at {:?}: {}", main_workdir, e))?;
        (main_repo, main_workdir)
    } else {
        let main_workdir = repo
            .workdir()
            .ok_or("Bare repository not supported")?
            .to_path_buf();
        (repo, main_workdir)
    };

    // Which branch (if any) is checked out in the main workdir right now.
    let main_branch: Option<String> = main_repo.head().ok().and_then(|h| {
        if h.is_branch() {
            h.shorthand().map(String::from)
        } else {
            None
        }
    });

    // Map: branch name → linked worktree path.
    let mut linked: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    if let Ok(worktrees) = main_repo.worktrees() {
        for wt_name in worktrees.iter().flatten() {
            let wt = match main_repo.find_worktree(wt_name) {
                Ok(w) => w,
                Err(_) => continue,
            };
            let wt_repo = match Repository::open_from_worktree(&wt) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // Extract the branch name as an owned String before wt_repo is
            // dropped so no Reference borrows outlive the iteration step.
            let branch_name: Option<String> = wt_repo.head().ok().and_then(|h| {
                if h.is_branch() {
                    h.shorthand().map(String::from)
                } else {
                    None
                }
            });
            if let Some(name) = branch_name {
                linked.insert(name, wt.path().to_path_buf());
            }
        }
    }

    let branches = main_repo
        .branches(Some(git2::BranchType::Local))
        .map_err(|e| format!("Failed to list branches: {}", e))?;

    let mut out = Vec::new();
    for entry in branches {
        let (branch, _) = match entry {
            Ok(b) => b,
            Err(_) => continue,
        };
        let name = match branch.name() {
            Ok(Some(n)) => n.to_string(),
            _ => continue,
        };
        let mut info = BranchInfo {
            name: name.clone(),
            worktree_path: None,
            is_project_head: false,
        };
        if main_branch.as_deref() == Some(name.as_str()) {
            info.worktree_path = Some(main_workdir.to_string_lossy().to_string());
            info.is_project_head = true;
        } else if let Some(path) = linked.get(&name) {
            info.worktree_path = Some(path.to_string_lossy().to_string());
        }
        out.push(info);
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Sanitize a branch name: allow alphanumeric, dashes, underscores, slashes, dots.
/// For filesystem paths (worktree dirs), slashes create subdirectories which is fine.
/// Dots are allowed (git permits them in ref names) but `..` path components are
/// rejected by the caller to prevent worktree path traversal.
fn sanitize_branch(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.') { c } else { '-' })
        .collect::<String>()
        .trim_matches(|c: char| c == '-' || c == '/')
        .to_string()
}

/// Reject branch names that would escape the `.sessonix-worktrees/` directory
/// when converted to a path. Covers `..`, `.`, leading/trailing dots per component.
fn is_safe_worktree_name(sanitized: &str) -> bool {
    if sanitized.is_empty() {
        return false;
    }
    // Any path component that is "." or ".." is unsafe.
    for component in sanitized.split('/') {
        if component == "." || component == ".." || component.is_empty() {
            return false;
        }
    }
    // After converting `/` to `-`, still shouldn't have `..`
    if sanitized.replace('/', "-").contains("..") {
        return false;
    }
    true
}

/// Create a git worktree for a new branch based on HEAD.
/// The worktree is placed under `<repo_root>/.sessonix-worktrees/<sanitized_branch>/`.
/// Auto-appends `.sessonix-worktrees/` to `.gitignore` if needed.
pub fn create_worktree(working_dir: &str, branch_name: &str) -> Result<WorktreeInfo, String> {
    let repo = Repository::discover(working_dir).map_err(|e| format!("Not a git repo: {}", e))?;
    let repo_root = repo.workdir().ok_or("Bare repository not supported")?.to_path_buf();

    // Get HEAD commit for branching
    let head = repo.head().map_err(|e| format!("No HEAD: {}", e))?;
    let head_commit = head.peel_to_commit().map_err(|e| format!("HEAD is not a commit: {}", e))?;
    let base_sha = format!("{}", head_commit.id());

    // Sanitize and deduplicate branch name
    let sanitized = sanitize_branch(branch_name);
    if sanitized.is_empty() {
        return Err(format!("Branch name '{}' produces an empty sanitized name", branch_name));
    }
    if !is_safe_worktree_name(&sanitized) {
        return Err(format!("Branch name '{}' contains unsafe path components (. or ..)", branch_name));
    }
    let mut final_branch = sanitized.clone();
    let mut suffix = 2;
    loop {
        let branch_exists = repo.find_branch(&final_branch, git2::BranchType::Local).is_ok();
        let dir_exists = repo_root.join(".sessonix-worktrees").join(final_branch.replace('/', "-")).exists();
        if !branch_exists && !dir_exists {
            break;
        }
        final_branch = format!("{}-{}", sanitized, suffix);
        suffix += 1;
    }

    // Create the branch
    repo.branch(&final_branch, &head_commit, false)
        .map_err(|e| format!("Failed to create branch '{}': {}", final_branch, e))?;

    // Worktree name for .git/worktrees/ must not contain slashes
    let wt_name = final_branch.replace('/', "-");
    let wt_dir = repo_root.join(".sessonix-worktrees").join(&wt_name);
    std::fs::create_dir_all(wt_dir.parent().unwrap())
        .map_err(|e| format!("Failed to create worktree dir: {}", e))?;

    // Add worktree via git2
    // We need to drop the reference before calling repo.worktree() because both borrow repo.
    // Use a block to scope the reference borrow.
    {
        let ref_name = format!("refs/heads/{}", final_branch);
        let reference = repo.find_reference(&ref_name)
            .map_err(|e| format!("Branch ref not found: {}", e))?;
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(&wt_name, &wt_dir, Some(&opts))
            .map_err(|e| format!("Failed to add worktree: {}", e))?;
    }

    // Auto-add to .gitignore
    let gitignore = repo_root.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if !content.contains(".sessonix-worktrees") {
                if let Err(e) = std::fs::write(&gitignore, format!("{}\n.sessonix-worktrees/\n", content.trim_end())) {
                    log::warn!("Could not update .gitignore: {}", e);
                }
            }
        }
    }

    Ok(WorktreeInfo {
        path: wt_dir.to_string_lossy().to_string(),
        branch: final_branch,
        base_commit: base_sha,
    })
}

/// Resolve the SHA of a branch tip without touching the filesystem.
/// Used when attaching a Task row to an already-existing worktree — we still
/// need a `base_commit` to store alongside the task.
pub fn branch_head_sha(working_dir: &str, branch_name: &str) -> Result<String, String> {
    let repo = Repository::discover(working_dir).map_err(|e| format!("Not a git repo: {}", e))?;
    let ref_name = format!("refs/heads/{}", branch_name);
    let reference = repo
        .find_reference(&ref_name)
        .map_err(|_| format!("Branch '{}' not found", branch_name))?;
    let commit = reference
        .peel_to_commit()
        .map_err(|e| format!("Branch '{}' does not point to a commit: {}", branch_name, e))?;
    Ok(format!("{}", commit.id()))
}

/// Create a worktree for an **existing** branch (no branch creation).
/// The worktree directory is placed under `<repo_root>/.sessonix-worktrees/<sanitized>/`.
/// Fails if `branch_name` doesn't exist locally or is already checked out elsewhere
/// (git2 rejects double-checkout — we surface the error).
pub fn create_worktree_from_branch(
    working_dir: &str,
    branch_name: &str,
) -> Result<WorktreeInfo, String> {
    let repo = Repository::discover(working_dir).map_err(|e| format!("Not a git repo: {}", e))?;
    let repo_root = repo
        .workdir()
        .ok_or("Bare repository not supported")?
        .to_path_buf();

    // Resolve the existing branch ref and its tip commit.
    let ref_name = format!("refs/heads/{}", branch_name);
    let reference = repo
        .find_reference(&ref_name)
        .map_err(|_| format!("Branch '{}' not found", branch_name))?;
    let head_commit = reference
        .peel_to_commit()
        .map_err(|e| format!("Branch '{}' does not point to a commit: {}", branch_name, e))?;
    let base_sha = format!("{}", head_commit.id());

    let sanitized = sanitize_branch(branch_name);
    if sanitized.is_empty() {
        return Err(format!(
            "Branch name '{}' produces an empty sanitized name",
            branch_name
        ));
    }
    if !is_safe_worktree_name(&sanitized) {
        return Err(format!(
            "Branch name '{}' contains unsafe path components (. or ..)",
            branch_name
        ));
    }

    // Only the worktree directory name is deduplicated — the branch itself is
    // kept as-is since we're attaching to it.
    let base_name = sanitized.replace('/', "-");
    let mut wt_name = base_name.clone();
    let mut suffix = 2;
    while repo_root.join(".sessonix-worktrees").join(&wt_name).exists() {
        wt_name = format!("{}-{}", base_name, suffix);
        suffix += 1;
    }

    let wt_dir = repo_root.join(".sessonix-worktrees").join(&wt_name);
    std::fs::create_dir_all(wt_dir.parent().unwrap())
        .map_err(|e| format!("Failed to create worktree dir: {}", e))?;

    {
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(&wt_name, &wt_dir, Some(&opts))
            .map_err(|e| format!("Failed to add worktree: {}", e))?;
    }

    let gitignore = repo_root.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if !content.contains(".sessonix-worktrees") {
                if let Err(e) = std::fs::write(
                    &gitignore,
                    format!("{}\n.sessonix-worktrees/\n", content.trim_end()),
                ) {
                    log::warn!("Could not update .gitignore: {}", e);
                }
            }
        }
    }

    Ok(WorktreeInfo {
        path: wt_dir.to_string_lossy().to_string(),
        branch: branch_name.to_string(),
        base_commit: base_sha,
    })
}

/// Remove a git worktree and its branch.
/// Prunes the worktree link, deletes the directory, and attempts to delete the branch.
pub fn remove_worktree(worktree_path: &str) -> Result<(), String> {
    let wt_path = Path::new(worktree_path);

    // If directory already deleted, try to infer parent repo from path
    // (.sessonix-worktrees/ is always inside the repo root)
    let discover_path = if wt_path.exists() {
        worktree_path.to_string()
    } else if let Some(parent) = wt_path.parent().and_then(|p| p.parent()) {
        // Go up from .sessonix-worktrees/<branch>/ to repo root
        parent.to_string_lossy().to_string()
    } else {
        return Ok(()); // Can't find repo, directory already gone, nothing to do
    };

    let repo = match Repository::discover(&discover_path) {
        Ok(r) => r,
        Err(_) => {
            // Can't find repo — just delete the directory if it exists
            if wt_path.exists() {
                std::fs::remove_dir_all(wt_path).ok();
            }
            return Ok(());
        }
    };

    // Find the worktree name by matching path
    let worktrees = repo.worktrees()
        .map_err(|e| format!("Cannot list worktrees: {}", e))?;

    let mut found_name: Option<String> = None;
    let canonical_target = std::fs::canonicalize(worktree_path).ok();
    for name in worktrees.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            // Canonicalize both paths to handle symlinks (macOS /var → /private/var)
            let canonical_wt = std::fs::canonicalize(wt.path()).ok();
            let matches = match (&canonical_wt, &canonical_target) {
                (Some(a), Some(b)) => a == b,
                _ => {
                    // Fallback to string comparison if canonicalize fails
                    wt.path().to_str().map(|s| s.trim_end_matches('/'))
                        == Some(worktree_path.trim_end_matches('/'))
                }
            };
            if matches {
                found_name = Some(name.to_string());
                break;
            }
        }
    }

    // Prune the worktree if found
    if let Some(ref name) = found_name {
        if let Ok(wt) = repo.find_worktree(name) {
            let mut opts = git2::WorktreePruneOptions::new();
            opts.valid(true);
            let _ = wt.prune(Some(&mut opts));
        }
    }

    // Delete the directory
    if wt_path.exists() {
        std::fs::remove_dir_all(wt_path)
            .map_err(|e| format!("Failed to remove worktree directory: {}", e))?;
    }

    // Try to delete the branch (best-effort).
    // repo.path() for a linked worktree is .git/worktrees/<name>/.
    // Go up to .git/, then to the main repo workdir.
    if let Some(ref name) = found_name {
        let main_workdir = repo.path()      // .git/worktrees/<name>/
            .parent()                        // .git/worktrees/
            .and_then(|p| p.parent())        // .git/
            .and_then(|p| p.parent());       // repo root
        if let Some(workdir) = main_workdir {
            if let Ok(main_repo) = Repository::open(workdir) {
                if let Ok(mut branch) = main_repo.find_branch(name, git2::BranchType::Local) {
                    let _ = branch.delete();
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let status = get_git_status(tmp.path().to_str().unwrap());
        assert!(!status.is_repo);
        assert!(status.branch.is_none());
        assert_eq!(status.changed_files, 0);
    }

    #[test]
    fn test_git_repo_status() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        // Create initial commit so HEAD exists
        let sig = repo.signature().unwrap_or_else(|_| {
            git2::Signature::now("Test", "test@test.com").unwrap()
        });
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();

        let status = get_git_status(tmp.path().to_str().unwrap());
        assert!(status.is_repo);
        assert!(status.branch.is_some()); // "main" or "master"
        assert_eq!(status.changed_files, 0);
        assert!(status.head_sha.is_some());
        assert!(!status.is_worktree);

        // Create a file to get changed_files > 0
        fs::write(tmp.path().join("test.txt"), "hello").unwrap();
        let status2 = get_git_status(tmp.path().to_str().unwrap());
        assert_eq!(status2.changed_files, 1);
    }

    #[test]
    fn test_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let sig = repo.signature().unwrap_or_else(|_| {
            git2::Signature::now("Test", "test@test.com").unwrap()
        });
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();

        let sub = tmp.path().join("src");
        fs::create_dir(&sub).unwrap();

        let status = get_git_status(sub.to_str().unwrap());
        assert!(status.is_repo);
    }

    fn init_repo_with_commit(path: &std::path::Path) -> Repository {
        let repo = Repository::init(path).unwrap();
        {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
        }
        repo
    }

    #[test]
    fn test_create_and_remove_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());

        let info = create_worktree(tmp.path().to_str().unwrap(), "aicoder/test-feature").unwrap();
        assert!(info.path.contains(".sessonix-worktrees"));
        assert!(!info.branch.is_empty());
        assert!(!info.base_commit.is_empty());
        assert!(std::path::Path::new(&info.path).exists());

        // Worktree should show as is_worktree
        let status = get_git_status(&info.path);
        assert!(status.is_repo);
        assert!(status.is_worktree);

        // Remove it
        remove_worktree(&info.path).unwrap();
        assert!(!std::path::Path::new(&info.path).exists());
    }

    #[test]
    fn test_branch_name_dedup() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());

        let info1 = create_worktree(tmp.path().to_str().unwrap(), "aicoder/dup").unwrap();
        let info2 = create_worktree(tmp.path().to_str().unwrap(), "aicoder/dup").unwrap();
        assert_ne!(info1.branch, info2.branch);
        assert!(info2.branch.contains("-2"));

        remove_worktree(&info1.path).unwrap();
        remove_worktree(&info2.path).unwrap();
    }

    #[test]
    fn test_reject_path_traversal_in_branch_name() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        // Plain `..` — should be rejected before any filesystem op.
        assert!(create_worktree(path, "..").is_err());
        // Path components with `..`
        assert!(create_worktree(path, "foo/../bar").is_err());
        assert!(create_worktree(path, "../escape").is_err());
        // Single `.` — also rejected (would be current dir)
        assert!(create_worktree(path, ".").is_err());
        // Embedded `..` after slash-to-dash conversion
        assert!(create_worktree(path, "a/../b").is_err());
    }

    #[test]
    fn test_safe_dotted_branch_names() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        // Single dots in normal places are fine (git allows dots in refs).
        let info = create_worktree(path, "feature/1.2.3").unwrap();
        assert!(!info.branch.is_empty());
        remove_worktree(&info.path).unwrap();
    }

    #[test]
    fn test_list_branches_marks_main_and_linked() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        // Extra branch without a worktree.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("topic", &head, false).unwrap();
        }

        // Extra branch with a linked worktree.
        let wt = create_worktree(path, "feature/linked").unwrap();

        let branches = list_branches(path).unwrap();
        let by_name: std::collections::HashMap<_, _> =
            branches.iter().map(|b| (b.name.clone(), b)).collect();

        // Main branch is named "main" or "master" depending on git defaults; look
        // it up via get_git_status.
        let status = get_git_status(path);
        let main_name = status.branch.unwrap();
        let main_info = by_name.get(&main_name).expect("main branch listed");
        assert!(main_info.is_project_head);
        assert!(main_info.worktree_path.is_some());

        let topic = by_name.get("topic").expect("topic listed");
        assert!(!topic.is_project_head);
        assert!(topic.worktree_path.is_none());

        let linked = by_name
            .get("feature/linked")
            .expect("linked branch listed");
        assert!(!linked.is_project_head);
        assert_eq!(linked.worktree_path.as_deref(), Some(wt.path.as_str()));

        remove_worktree(&wt.path).unwrap();
    }

    #[test]
    fn test_list_branches_from_inside_linked_worktree() {
        // Ensure we still see all branches (including the main checkout) when
        // `working_dir` is a linked worktree instead of the main workdir.
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        let wt = create_worktree(path, "feature/wt").unwrap();
        let branches = list_branches(&wt.path).unwrap();

        let status = get_git_status(path);
        let main_name = status.branch.unwrap();
        let by_name: std::collections::HashMap<_, _> =
            branches.iter().map(|b| (b.name.clone(), b)).collect();
        assert!(by_name.get(&main_name).unwrap().is_project_head);
        assert_eq!(
            by_name
                .get("feature/wt")
                .unwrap()
                .worktree_path
                .as_deref(),
            Some(wt.path.as_str())
        );

        remove_worktree(&wt.path).unwrap();
    }

    #[test]
    fn test_create_worktree_from_existing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        // Create a branch without a worktree.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("release/1.0", &head, false).unwrap();
        }

        let info = create_worktree_from_branch(path, "release/1.0").unwrap();
        assert_eq!(info.branch, "release/1.0");
        assert!(info.path.contains(".sessonix-worktrees"));
        assert!(std::path::Path::new(&info.path).exists());
        assert!(!info.base_commit.is_empty());

        // Branch should now show up as linked in list_branches.
        let branches = list_branches(path).unwrap();
        let entry = branches
            .iter()
            .find(|b| b.name == "release/1.0")
            .expect("branch listed");
        assert_eq!(entry.worktree_path.as_deref(), Some(info.path.as_str()));

        remove_worktree(&info.path).unwrap();
    }

    #[test]
    fn test_create_worktree_from_branch_rejects_missing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let err = create_worktree_from_branch(tmp.path().to_str().unwrap(), "does-not-exist")
            .expect_err("missing branch must fail");
        assert!(err.contains("not found"), "unexpected error: {}", err);
    }

    #[test]
    fn test_create_worktree_from_branch_rejects_already_checked_out() {
        // Attaching a new worktree to a branch that is already checked out in
        // another linked worktree should fail — git2 refuses double-checkout.
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());
        let path = tmp.path().to_str().unwrap();

        let wt = create_worktree(path, "feature/busy").unwrap();
        let err = create_worktree_from_branch(path, "feature/busy")
            .expect_err("branch is already in a worktree");
        assert!(
            err.to_lowercase().contains("worktree") || err.to_lowercase().contains("already"),
            "unexpected error: {}",
            err
        );

        remove_worktree(&wt.path).unwrap();
    }

    #[test]
    fn test_gitignore_auto_append() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_commit(tmp.path());

        // Create a .gitignore without the worktree entry
        fs::write(tmp.path().join(".gitignore"), "target/\n").unwrap();

        let info = create_worktree(tmp.path().to_str().unwrap(), "aicoder/ignore-test").unwrap();
        let gitignore = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".sessonix-worktrees/"));

        remove_worktree(&info.path).unwrap();
    }
}
