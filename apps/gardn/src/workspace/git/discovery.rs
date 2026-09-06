use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeInfo {
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
    pub git_common_dir: PathBuf,
    pub is_bare: bool,
}

pub fn derive_label_from_cwd(cwd: &Path) -> String {
    if let Some(repo_root) = git_repo_root(cwd) {
        if let Some(name) = repo_root.file_name().and_then(|n| n.to_str()) {
            return name.to_string();
        }
    }

    fallback_label_from_cwd(cwd)
}

pub fn fallback_label_from_cwd(cwd: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home = Path::new(&home);
        if cwd == home {
            return "~".to_string();
        }
    }

    cwd.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| cwd.display().to_string())
}

pub fn derive_label_from_location(location: &crate::execution_host::ResourceLocation) -> String {
    if location.is_local() {
        return derive_label_from_cwd(location.path.as_path());
    }
    let path = location.path.as_path();
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

pub fn git_worktree_info(cwd: &Path) -> Option<GitWorktreeInfo> {
    let repo_root = git_repo_root(cwd)?;
    let git_dir = canonicalize_best_effort_path(&git_dir_for_repo_root(&repo_root)?);
    let git_common_dir = canonicalize_best_effort_path(&git_common_dir_for_git_dir(&git_dir));
    let is_bare = git_dir_is_bare(&git_dir);

    Some(GitWorktreeInfo {
        repo_root,
        git_dir,
        git_common_dir,
        is_bare,
    })
}

pub(super) fn canonicalize_best_effort_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn git_common_dir_for_git_dir(git_dir: &Path) -> PathBuf {
    let commondir = git_dir.join("commondir");
    let Ok(contents) = std::fs::read_to_string(commondir) else {
        return git_dir.to_path_buf();
    };
    let path = Path::new(contents.trim());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        git_dir.join(path)
    }
}
pub fn git_branch(cwd: &Path) -> Option<String> {
    let repo_root = git_repo_root(cwd)?;
    let git_dir = git_dir_for_repo_root(&repo_root)?;
    let git_common_dir = git_common_dir_for_git_dir(&git_dir);
    if git_ref_storage_is_reftable(&git_common_dir) {
        return git_symbolic_head_short(&repo_root);
    }

    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    parse_git_head_branch(&head)
}

pub(super) fn git_dir_for_repo_root(repo_root: &Path) -> Option<PathBuf> {
    let git_path = repo_root.join(".git");
    if git_path.is_dir() {
        return Some(git_path);
    }

    if let Ok(gitdir) = std::fs::read_to_string(&git_path) {
        if let Some(relative) = gitdir.trim().strip_prefix("gitdir:").map(str::trim) {
            let resolved = Path::new(relative);
            return Some(if resolved.is_absolute() {
                resolved.to_path_buf()
            } else {
                repo_root.join(resolved)
            });
        }
    }

    if path_is_git_dir_layout(repo_root) && git_dir_is_bare(repo_root) {
        return Some(repo_root.to_path_buf());
    }

    None
}

fn path_is_git_dir_layout(path: &Path) -> bool {
    path.join("HEAD").is_file() && path.join("objects").is_dir() && path.join("refs").is_dir()
}

pub(super) fn git_symbolic_head_full(repo_root: &Path) -> Option<String> {
    git_trimmed_stdout(repo_root, &["symbolic-ref", "--quiet", "HEAD"])
}

fn git_symbolic_head_short(repo_root: &Path) -> Option<String> {
    git_trimmed_stdout(repo_root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
}

pub(super) fn git_rev_parse_verify(repo_root: &Path, revision: &str) -> Option<String> {
    git_trimmed_stdout(repo_root, &["rev-parse", "--verify", revision])
}

pub(super) fn git_ref_storage_is_reftable(git_common_dir: &Path) -> bool {
    read_git_config_value(&git_common_dir.join("config"), "extensions", "refstorage")
        .is_some_and(|value| value.eq_ignore_ascii_case("reftable"))
}

fn git_dir_is_bare(git_dir: &Path) -> bool {
    read_git_config_value(&git_dir.join("config"), "core", "bare")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn parse_git_head_branch(head: &str) -> Option<String> {
    let branch = head.trim().strip_prefix("ref: refs/heads/")?;
    (!branch.is_empty()).then(|| branch.to_string())
}

fn read_git_config_value(path: &Path, section: &str, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section_name) = simple_git_config_section(line) {
            in_section = section_name.eq_ignore_ascii_case(section);
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case(key) {
            return Some(strip_git_config_comment(value).trim().to_string());
        }
    }
    None
}

fn simple_git_config_section(line: &str) -> Option<&str> {
    let section = line.strip_prefix('[')?.split_once(']')?.0.trim();
    (!section.contains('"')).then_some(section)
}

fn strip_git_config_comment(value: &str) -> &str {
    let value = value.trim();
    for marker in ['#', ';'] {
        if let Some((prefix, _)) = value.split_once(marker) {
            if prefix.chars().next_back().is_some_and(char::is_whitespace) {
                return prefix;
            }
        }
    }
    value
}
fn parse_github_origin(origin: &str) -> Option<crate::github::GithubRepository> {
    let origin = origin.trim();
    let path = [
        "git@github.com:",
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|prefix| origin.strip_prefix(prefix))?;
    crate::github::GithubRepository::parse(path).ok()
}

pub(crate) fn discover_github_repositories(
    cwds: &[PathBuf],
) -> crate::github::GithubDiscoveryOutcome {
    use crate::github::GithubDiscoveryOutcome;

    let mut roots = std::collections::BTreeSet::new();
    for cwd in cwds {
        match github_repo_root(cwd) {
            Ok(Some(root)) => {
                roots.insert(root);
            }
            Ok(None) => {}
            Err(error) => return GithubDiscoveryOutcome::Failed(error),
        }
    }
    let mut repositories = std::collections::BTreeSet::new();
    for root in roots {
        match github_repository_for_repo_root(&root) {
            Ok(Some(repository)) => {
                repositories.insert(repository);
            }
            Ok(None) => {}
            Err(error) => return GithubDiscoveryOutcome::Failed(error),
        }
    }
    if repositories.is_empty() {
        GithubDiscoveryOutcome::Empty
    } else {
        GithubDiscoveryOutcome::Repositories(repositories.into_iter().collect())
    }
}

fn github_repo_root(cwd: &Path) -> Result<Option<PathBuf>, String> {
    let failure = |error: std::io::Error| {
        format!(
            "failed to discover GitHub repository for {}: {error}",
            cwd.display()
        )
    };
    let mut current = std::fs::canonicalize(cwd).map_err(failure)?;
    loop {
        let mut has_git = false;
        let mut has_objects = false;
        let mut has_refs = false;
        for entry in std::fs::read_dir(&current).map_err(failure)? {
            let name = entry.map_err(failure)?.file_name();
            has_git |= name == ".git";
            has_objects |= name == "objects";
            has_refs |= name == "refs";
        }
        if has_git || (has_objects && has_refs) {
            let git_dir = if has_git {
                current.join(".git")
            } else {
                current.clone()
            };
            let output = crate::noninteractive_process::command("git")
                .arg("--git-dir")
                .arg(&git_dir)
                .args(["rev-parse", "--is-bare-repository"])
                .output()
                .map_err(failure)?;
            if !output.status.success() {
                return Err(format!(
                    "failed to inspect Git repository metadata at {}: {}",
                    git_dir.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn github_repository_for_repo_root(
    repo_root: &Path,
) -> Result<Option<crate::github::GithubRepository>, String> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|error| {
            format!(
                "failed to read GitHub origin for {}: {error}",
                repo_root.display()
            )
        })?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            stderr
        };
        return Err(format!(
            "failed to read GitHub origin for {}: {detail}",
            repo_root.display()
        ));
    }

    let origin = String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "GitHub origin for {} was not UTF-8: {error}",
            repo_root.display()
        )
    })?;
    Ok(parse_github_origin(&origin))
}

fn git_trimmed_stdout(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = crate::noninteractive_process::command("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let stdout = stdout.trim();
    (!stdout.is_empty()).then(|| stdout.to_string())
}

pub(crate) fn git_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        if git_dir_for_repo_root(&current)
            .map(|git_dir| git_dir.join("HEAD").is_file())
            .unwrap_or(false)
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub(super) fn read_ref_oid(common_dir: &Path, full_ref: &str) -> Option<String> {
    let loose_ref = common_dir.join(full_ref);
    if let Ok(contents) = std::fs::read_to_string(loose_ref) {
        let oid = contents.trim();
        if !oid.is_empty() {
            return Some(oid.to_string());
        }
    }

    let packed_refs = std::fs::read_to_string(common_dir.join("packed-refs")).ok()?;
    for line in packed_refs.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let oid = parts.next()?;
        let name = parts.next()?;
        if name == full_ref {
            return Some(oid.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::workspace::git::test_support::run_git;

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = format!(
            "gardn-workspace-tests-{}-{}-{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn github_origin_parser_accepts_supported_forms_only() {
        for origin in [
            "git@github.com:Acme/One.git",
            "https://github.com/Acme/One",
            "http://github.com/Acme/One.git",
            "ssh://git@github.com/Acme/One",
        ] {
            assert_eq!(
                parse_github_origin(origin).expect("GitHub origin"),
                crate::github::GithubRepository::parse("acme/one").unwrap()
            );
        }
        for origin in [
            "git@gitlab.com:acme/one.git",
            "https://github.com/acme/one/issues",
            "ssh://git@github.com/acme/one?ref=head",
        ] {
            assert_eq!(parse_github_origin(origin), None);
        }
    }

    #[test]
    fn github_discovery_rejects_corrupt_metadata_without_broadening_scope() {
        use crate::github::{resolve_github_scope, GithubDiscoveryOutcome, GithubRepositoryScope};

        for corruption in ["missing-head", "unreadable-head", "gitfile", "commondir"] {
            let root = temp_test_dir(corruption);
            run_git(&root, &["init", "-b", "main"]);
            run_git(
                &root,
                &["remote", "add", "origin", "https://github.com/acme/parent"],
            );
            let checkout = root.join("checkout");
            std::fs::create_dir_all(&checkout).unwrap();
            run_git(&checkout, &["init", "-b", "main"]);
            match corruption {
                "missing-head" => std::fs::remove_file(checkout.join(".git/HEAD")).unwrap(),
                "unreadable-head" => {
                    std::fs::remove_file(checkout.join(".git/HEAD")).unwrap();
                    std::fs::create_dir(checkout.join(".git/HEAD")).unwrap();
                }
                "gitfile" => {
                    std::fs::remove_dir_all(checkout.join(".git")).unwrap();
                    std::fs::write(checkout.join(".git"), "not a gitdir pointer\n").unwrap();
                }
                "commondir" => {
                    std::fs::write(checkout.join(".git/commondir"), "missing\n").unwrap();
                }
                _ => unreachable!(),
            }
            let discovery = discover_github_repositories(&[checkout]);
            assert!(
                matches!(discovery, GithubDiscoveryOutcome::Failed(_)),
                "{corruption}: {discovery:?}"
            );
            let org = crate::app::state::GithubOrganization::parse("acme")
                .unwrap()
                .unwrap();
            assert!(resolve_github_scope(
                &GithubRepositoryScope::Automatic,
                &discovery,
                Some(&org)
            )
            .is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn github_discovery_rejects_missing_working_directory() {
        let root = temp_test_dir("github-missing-cwd");
        assert!(matches!(
            discover_github_repositories(&[root.join("missing")]),
            crate::github::GithubDiscoveryOutcome::Failed(_)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_branch_reads_head_from_standard_repo() {
        let root = temp_test_dir("standard-repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        assert_eq!(git_branch(&root).as_deref(), Some("main"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_branch_reads_head_from_worktree_gitdir_file() {
        let root = temp_test_dir("worktree");
        let worktree_git_dir = root.join(".bare/worktrees/feature");
        std::fs::create_dir_all(&worktree_git_dir).unwrap();
        std::fs::write(root.join(".git"), "gitdir: .bare/worktrees/feature\n").unwrap();
        std::fs::write(worktree_git_dir.join("HEAD"), "ref: refs/heads/feature\n").unwrap();

        assert_eq!(git_branch(&root).as_deref(), Some("feature"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_branch_returns_none_for_detached_head() {
        let root = temp_test_dir("detached-head");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "3e1b9a8d\n").unwrap();

        assert_eq!(git_branch(&root), None);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_branch_reads_symbolic_head_from_reftable_repo() {
        let root = temp_test_dir("reftable-branch");
        let root_arg = root.to_string_lossy().to_string();
        let output = std::process::Command::new("git")
            .args(["init", "--ref-format=reftable", "-b", "main", &root_arg])
            .output()
            .unwrap();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            std::fs::remove_dir_all(root).unwrap();
            panic!("git init --ref-format=reftable failed: {stderr}");
        }

        assert_eq!(git_branch(&root).as_deref(), Some("main"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_repo_root_ignores_invalid_git_marker() {
        let base = temp_test_dir("invalid-git-root");
        let cwd = base.join("workspace");
        std::fs::create_dir_all(base.join(".git")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        assert_eq!(git_repo_root(&cwd), None);

        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn git_repo_root_ignores_standalone_non_bare_git_dir_layout() {
        let root = temp_test_dir("standalone-non-bare-git-dir");
        std::fs::write(root.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(root.join("objects")).unwrap();
        std::fs::create_dir_all(root.join("refs")).unwrap();
        std::fs::write(root.join("config"), "[core]\n\tbare = false\n").unwrap();

        assert_eq!(git_repo_root(&root.join("refs")), None);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn derive_label_prefers_repo_root_name() {
        let root = temp_test_dir("label-repo");
        let nested = root.join("nested");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            derive_label_from_cwd(&nested),
            root.file_name().and_then(|name| name.to_str()).unwrap()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn derive_label_uses_path_name_outside_git() {
        let root = temp_test_dir("label-plain");
        let label = root.file_name().and_then(|name| name.to_str()).unwrap();

        assert_eq!(derive_label_from_cwd(Path::new(&root)), label);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remote_label_does_not_inspect_coordinator_filesystem() {
        let location = crate::execution_host::ResourceLocation::new(
            crate::execution_host::ExecutionHostId::new("ssh:workbox").expect("remote host id"),
            crate::execution_host::HostPath::new("/srv/project/nested").expect("remote path"),
        );

        assert_eq!(derive_label_from_location(&location), "nested");
    }

    #[test]
    fn git_rev_parse_verify_reads_reftable_refs() {
        let root = temp_test_dir("reftable-ref-oid");
        let root_arg = root.to_string_lossy().to_string();
        let output = std::process::Command::new("git")
            .args(["init", "--ref-format=reftable", "-b", "main", &root_arg])
            .output()
            .unwrap();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            std::fs::remove_dir_all(root).unwrap();
            panic!("git init --ref-format=reftable failed: {stderr}");
        }

        run_git(&root, &["config", "user.email", "gardn@example.invalid"]);
        run_git(&root, &["config", "user.name", "Gardn Test"]);
        run_git(&root, &["commit", "--allow-empty", "-m", "initial"]);

        let head_oid = git_rev_parse_verify(&root, "HEAD").unwrap();

        assert_eq!(
            git_rev_parse_verify(&root, "refs/heads/main").as_deref(),
            Some(head_oid.as_str())
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
