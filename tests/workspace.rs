use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Repo {
    dir: PathBuf,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "shifu-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = dir.join("repo with 空格");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(root.join("src.txt"), "original\n").unwrap();
        fs::write(root.join(".gitignore"), "/target\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "initial"]);
        Self { dir, root }
    }
    fn tree(&self, name: &str) -> PathBuf {
        let path = self.dir.join(name);
        json(
            &self.root,
            &[
                "create",
                name,
                path.to_str().unwrap(),
                "--goal",
                "修改指定文件",
                "--scope",
                "src.txt",
                "--json",
            ],
        );
        path
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shifu"))
        .arg("workspace")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn json(cwd: &Path, args: &[&str]) -> Value {
    let out = run(cwd, args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn real_worktree_handoff_keeps_versions_and_references_without_copying_payloads() {
    let repo = Repo::new();
    fs::write(repo.root.join("src.txt"), "uncommitted main work\n").unwrap();
    fs::create_dir(repo.root.join("target")).unwrap();
    fs::write(repo.root.join("target/large.bin"), vec![1; 1024 * 1024]).unwrap();
    let tree = repo.tree("worker-a");
    assert_eq!(
        git(
            &repo.root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"]
        ),
        git(
            &tree,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"]
        )
    );
    assert_eq!(
        fs::read_to_string(tree.join("src.txt")).unwrap(),
        "original\n"
    );
    assert!(!tree.join("target").exists());
    let initial = json(&tree, &["brief", "--json"]);
    assert_eq!(initial["task"]["goal"], "修改指定文件");
    assert_eq!(initial["task"]["scope"][0], "src.txt");
    assert_eq!(initial["branch"], "shifu/worker-a");
    assert_eq!(initial["head"], initial["task"]["base_commit"]);
    fs::write(tree.join("src.txt"), "agent A result\n").unwrap();
    json(
        &tree,
        &[
            "handoff",
            "--done",
            "已修改 src.txt",
            "--next",
            "检查行为，再提交",
            "--artifact",
            "src.txt",
            "--json",
        ],
    );
    let next = json(
        &repo.root,
        &["brief", "--path", tree.to_str().unwrap(), "--json"],
    );
    assert_eq!(next["task"]["done"], "已修改 src.txt");
    assert_eq!(next["task"]["next"], "检查行为，再提交");
    assert_eq!(next["task"]["artifacts"][0], "src.txt");
    assert!(
        next["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("src.txt"))
    );
    assert_eq!(
        fs::read_to_string(repo.root.join("src.txt")).unwrap(),
        "uncommitted main work\n"
    );
    assert!(
        !String::from_utf8(run(&tree, &["brief"]).stdout)
            .unwrap()
            .contains("agent A result")
    );
    let listed = json(&repo.root, &["list", "--json"]);
    assert_eq!(listed.as_array().unwrap().len(), 2);
}

#[test]
fn reclamation_plan_preserves_dirty_unmerged_locked_and_ignored_work() {
    let repo = Repo::new();
    let clean = repo.tree("clean");
    let dirty = repo.tree("dirty");
    fs::write(dirty.join("draft.txt"), "keep this draft").unwrap();
    let ignored = repo.tree("ignored");
    fs::create_dir(ignored.join("target")).unwrap();
    fs::write(
        ignored.join("target/draft.txt"),
        "ignored is not disposable",
    )
    .unwrap();
    let unmerged = repo.tree("unmerged");
    fs::write(unmerged.join("src.txt"), "new feature").unwrap();
    git(&unmerged, &["add", "src.txt"]);
    git(&unmerged, &["commit", "-m", "unmerged feature"]);
    let locked = repo.tree("locked");
    git(&repo.root, &["worktree", "lock", locked.to_str().unwrap()]);
    let plan = json(&repo.root, &["plan", "--base", "main", "--json"]);
    let row = |path: &Path| {
        plan.as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"].as_str() == path.canonicalize().unwrap().to_str())
            .unwrap()
    };
    assert_eq!(row(&clean)["disposition"], "review");
    for (path, reason) in [
        (&dirty, "changes"),
        (&ignored, "ignored-files"),
        (&unmerged, "unmerged"),
        (&locked, "locked"),
        (&repo.root, "primary"),
    ] {
        let r = row(path);
        assert_eq!(r["disposition"], "keep");
        assert!(
            r["reasons"].as_array().unwrap().iter().any(|v| v == reason),
            "{r}"
        );
    }
    assert_eq!(
        fs::read_to_string(ignored.join("target/draft.txt")).unwrap(),
        "ignored is not disposable"
    );
    assert_eq!(
        git(&repo.root, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        6
    );
}

#[test]
fn handoff_refuses_missing_task_and_paths_that_escape_the_workspace() {
    let repo = Repo::new();
    assert!(
        !run(&repo.root, &["handoff", "--done", "x", "--next", "y"])
            .status
            .success()
    );
    let invalid = run(
        &repo.root,
        &[
            "create",
            "bad",
            repo.dir.join("bad").to_str().unwrap(),
            "--goal",
            "x",
            "--scope",
            "../outside",
        ],
    );
    assert!(!invalid.status.success());
    assert!(!repo.dir.join("bad").exists());
    let tree = repo.tree("safe");
    fs::write(repo.dir.join("outside"), "secret").unwrap();
    let out = run(
        &tree,
        &[
            "handoff",
            "--done",
            "x",
            "--next",
            "y",
            "--artifact",
            "../outside",
        ],
    );
    assert!(!out.status.success());
    assert_eq!(json(&tree, &["brief", "--json"])["task"]["next"], "");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(repo.dir.join("outside"), tree.join("outside-link")).unwrap();
        assert!(
            !run(
                &tree,
                &[
                    "handoff",
                    "--done",
                    "x",
                    "--next",
                    "y",
                    "--artifact",
                    "outside-link"
                ]
            )
            .status
            .success()
        );
    }
}

#[test]
fn invalid_base_and_unknown_options_never_create_a_workspace() {
    let repo = Repo::new();
    let dest = repo.dir.join("never");
    let args = [
        "create",
        "never",
        dest.to_str().unwrap(),
        "--goal",
        "x",
        "--scope",
        "src.txt",
        "--base",
        "nonexistent",
    ];
    assert!(!run(&repo.root, &args).status.success());
    assert!(!dest.exists());
    assert!(!run(&repo.root, &["plan", "--force"]).status.success());
    assert!(!run(&repo.dir, &["brief"]).status.success());
}

#[test]
fn source_workspaces_stay_outside_build_output_and_notes_remain_small() {
    let repo = Repo::new();
    let dest = repo.root.join("target/source");
    assert!(
        !run(
            &repo.root,
            &[
                "create",
                "nested",
                dest.to_str().unwrap(),
                "--goal",
                "x",
                "--scope",
                "src.txt"
            ]
        )
        .status
        .success()
    );
    assert!(!dest.exists());
    let tree = repo.tree("small");
    let huge = "x".repeat(17 * 1024);
    assert!(
        !run(&tree, &["handoff", "--done", &huge, "--next", "continue"])
            .status
            .success()
    );
    assert_eq!(json(&tree, &["brief", "--json"])["task"]["done"], "");
    let before = json(&tree, &["brief", "--json"])["head"].clone();
    fs::write(tree.join("src.txt"), "another version").unwrap();
    git(&tree, &["add", "src.txt"]);
    git(&tree, &["commit", "-m", "another version"]);
    let current = json(&tree, &["brief", "--json"]);
    assert_ne!(current["head"], before);
    assert_eq!(current["task"]["saved_head"], before);
    assert!(
        String::from_utf8(run(&tree, &["brief"]).stdout)
            .unwrap()
            .contains("提交已变化")
    );
}

#[test]
fn existing_checkout_can_keep_one_task_note_without_creating_a_new_worktree() {
    let repo = Repo::new();
    json(
        &repo.root,
        &[
            "init",
            "--goal",
            "接续现有任务",
            "--scope",
            "src.txt",
            "--json",
        ],
    );
    json(
        &repo.root,
        &[
            "handoff",
            "--done",
            "材料已看",
            "--next",
            "修复后检查",
            "--json",
        ],
    );
    let brief = json(&repo.root, &["brief", "--json"]);
    assert_eq!(brief["task"]["goal"], "接续现有任务");
    assert_eq!(brief["task"]["next"], "修复后检查");
    assert_eq!(git(&repo.root, &["status", "--porcelain"]), "");
    assert_eq!(
        json(&repo.root, &["list", "--json"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        !run(
            &repo.root,
            &["init", "--goal", "覆盖", "--scope", "src.txt"]
        )
        .status
        .success()
    );
    assert_eq!(
        json(&repo.root, &["brief", "--json"])["task"]["goal"],
        "接续现有任务"
    );
}

fn commit_all(tree: &Path, message: &str) {
    git(tree, &["add", "-A"]);
    git(tree, &["commit", "-q", "-m", message]);
}

fn ready(tree: &Path) {
    json(
        tree,
        &["handoff", "--done", "已提交", "--next", "等合并", "--ready", "--json"],
    );
}

fn today() -> String {
    let out = Command::new("date").arg("+%m-%d").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

impl Repo {
    fn scoped(&self, name: &str, scope: &str) -> (PathBuf, Output) {
        let path = self.dir.join(name);
        let out = run(
            &self.root,
            &[
                "create",
                name,
                path.to_str().unwrap(),
                "--goal",
                "并行修改",
                "--scope",
                scope,
                "--json",
            ],
        );
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        (path, out)
    }
    fn with_changelog(self) -> Self {
        fs::write(
            self.root.join("CHANGELOG.md"),
            "# 改动记录\n\n说明。\n\n## 01-01\n\n- 旧条目\n",
        )
        .unwrap();
        commit_all(&self.root, "changelog");
        self
    }
}

#[test]
fn creating_a_workspace_warns_when_an_active_one_already_covers_the_same_files() {
    let repo = Repo::new();
    let (_a, _) = repo.scoped("style-a", "app/src");
    let (_b, out) = repo.scoped("style-b", "app/src/style.css");
    let warning = String::from_utf8_lossy(&out.stderr);
    assert!(warning.contains("shifu/style-a"), "{warning}");
    assert!(warning.contains("app/src"), "{warning}");
    let (_c, quiet) = repo.scoped("docs-c", "docs");
    assert!(quiet.stderr.is_empty(), "{}", String::from_utf8_lossy(&quiet.stderr));
}

#[test]
fn merge_lands_every_ready_branch_and_folds_their_changelog_notes_without_touching_main_edits() {
    let repo = Repo::new().with_changelog();
    let (a, _) = repo.scoped("feat-a", "a.txt");
    let (b, _) = repo.scoped("feat-b", "b.txt");
    let (c, _) = repo.scoped("feat-c", "c.txt");
    for (tree, file, note) in [(&a, "a.txt", "- 新增 A"), (&b, "b.txt", "- 新增 B")] {
        fs::write(tree.join(file), "work\n").unwrap();
        fs::create_dir_all(tree.join("changelog.d")).unwrap();
        let branch = git(tree, &["branch", "--show-current"]).replace('/', "-");
        fs::write(tree.join(format!("changelog.d/{branch}.md")), format!("{note}\n")).unwrap();
        commit_all(tree, file);
        ready(tree);
    }
    fs::write(c.join("c.txt"), "not ready\n").unwrap();
    commit_all(&c, "c");
    // Someone else's uncommitted edit in the main checkout must survive the merge.
    fs::write(repo.root.join("src.txt"), "uncommitted main work\n").unwrap();

    let report = json(&repo.root, &["merge", "--json"]);
    let merged: Vec<_> = report["merged"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(merged, ["shifu/feat-a", "shifu/feat-b"]);
    assert_eq!(report["main_updated"], true);
    assert!(repo.root.join("a.txt").exists() && repo.root.join("b.txt").exists());
    assert!(!repo.root.join("c.txt").exists());
    assert_eq!(fs::read_to_string(repo.root.join("src.txt")).unwrap(), "uncommitted main work\n");
    let log = fs::read_to_string(repo.root.join("CHANGELOG.md")).unwrap();
    let head = format!("## {}\n", today());
    let at = log.find(&head).expect("today's heading");
    assert!(at < log.find("## 01-01").unwrap());
    assert!(log[at..].contains("- 新增 A") && log[at..].contains("- 新增 B"), "{log}");
    assert!(!repo.root.join("changelog.d").join("shifu-feat-a.md").exists());
    // Running again finds nothing new to merge.
    let again = json(&repo.root, &["merge", "--json"]);
    assert!(again["merged"].as_array().unwrap().is_empty());
}

#[test]
fn merge_skips_conflicts_failed_checks_and_branches_changed_after_ready() {
    let repo = Repo::new();
    let (a, _) = repo.scoped("first", "src.txt");
    let (b, _) = repo.scoped("second", "src.txt");
    let (c, _) = repo.scoped("moved", "c.txt");
    let (d, _) = repo.scoped("broken", "d.txt");
    for (tree, text) in [(&a, "from A\n"), (&b, "from B\n")] {
        fs::write(tree.join("src.txt"), text).unwrap();
        commit_all(tree, "edit src");
        ready(tree);
    }
    fs::write(c.join("c.txt"), "v1\n").unwrap();
    commit_all(&c, "c1");
    ready(&c);
    fs::write(c.join("c.txt"), "v2\n").unwrap();
    commit_all(&c, "c2 after ready");
    fs::write(d.join("d.txt"), "bad\n").unwrap();
    commit_all(&d, "d");
    ready(&d);

    let report = json(&repo.root, &["merge", "--check", "! grep -q bad d.txt 2>/dev/null", "--json"]);
    let names = |k: &str| -> Vec<String> {
        report[k].as_array().unwrap().iter().map(|v| v["branch"].as_str().unwrap_or(v.as_str().unwrap_or("")).to_owned()).collect()
    };
    assert_eq!(report["merged"].as_array().unwrap().len(), 1);
    assert!(names("conflicts").iter().any(|n| n == "shifu/second") || names("conflicts").iter().any(|n| n == "shifu/first"));
    assert_eq!(report["conflicts"][0]["files"][0], "src.txt");
    assert_eq!(names("changed_after_ready"), ["shifu/moved"]);
    assert_eq!(names("check_failed"), ["shifu/broken"]);
    assert!(!repo.root.join("d.txt").exists());
    assert_eq!(git(&repo.root, &["status", "--porcelain"]), "");
}

#[test]
fn named_branches_can_be_merged_without_a_ready_mark() {
    let repo = Repo::new();
    git(&repo.root, &["branch", "old/feature"]);
    let old = repo.dir.join("old");
    git(&repo.root, &["worktree", "add", "-q", old.to_str().unwrap(), "old/feature"]);
    fs::write(old.join("old.txt"), "legacy\n").unwrap();
    commit_all(&old, "legacy");
    let report = json(&repo.root, &["merge", "--branch", "old/feature", "--json"]);
    assert_eq!(report["merged"][0], "old/feature");
    assert!(repo.root.join("old.txt").exists());
}

#[test]
fn changelog_notes_follow_the_existing_heading_style_and_new_workspaces_get_the_private_rules_pointer() {
    let repo = Repo::new();
    fs::write(
        repo.root.join("CHANGELOG.md"),
        "# Change log\n\n## Source update — 2001-01-01 (not in preview downloads)\n\n- old\n",
    )
    .unwrap();
    commit_all(&repo.root, "changelog");
    let rules = repo.dir.join("private-rules.md");
    fs::write(&rules, "# rules\n").unwrap();
    git(&repo.root, &["config", "shifu.rules", rules.to_str().unwrap()]);
    let (a, _) = repo.scoped("feat-a", "a.txt");
    let agents = fs::read_to_string(a.join("AGENTS.md")).unwrap();
    assert!(agents.contains(rules.to_str().unwrap()), "{agents}");
    assert!(fs::read_to_string(a.join("CLAUDE.md")).unwrap().contains(rules.to_str().unwrap()));
    // The pointer files stay out of commits.
    assert_eq!(git(&a, &["status", "--porcelain"]), "");
    fs::write(a.join("a.txt"), "work\n").unwrap();
    fs::create_dir_all(a.join("changelog.d")).unwrap();
    fs::write(a.join("changelog.d/shifu-feat-a.md"), "- 新增 A\n- Adds A\n").unwrap();
    commit_all(&a, "a");
    ready(&a);
    json(&repo.root, &["merge", "--json"]);
    let log = fs::read_to_string(repo.root.join("CHANGELOG.md")).unwrap();
    let out = Command::new("date").arg("+%Y-%m-%d").output().unwrap();
    let day = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let heading = format!("## Source update — {day} (not in preview downloads)\n\n- 新增 A\n- Adds A\n");
    assert!(log.contains(&heading), "{log}");
    assert!(log.find(&heading).unwrap() < log.find("2001-01-01").unwrap());
}
