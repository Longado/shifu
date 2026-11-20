//! Git owns versions and worktrees. Shifu adds a bounded task note, references, and a read-only retirement plan.

use crate::agent::Res;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const NOTE: &str = "shifu-task.json";
const NOTE_LIMIT: usize = 16 * 1024;
const GUIDE: &str = "shifu workspace — 共用 Git 历史，分别修改文件，按需读取接续说明

  list [--json]                         列出当前仓库所有 worktree 及登记的目标，没登记的会标出
  init --goal <目标> --scope <相对路径> [--scope ...] [--json]
                                        在现有工作区登记任务，不另建工作区，不覆盖已有记录
  create <名字> <目录> --goal <目标> --scope <相对路径> [--scope ...] [--base <提交>] [--json]
                                        从已提交版本建 shifu/<名字> 分支；不复制缓存或未提交文件
  brief [--path <工作区>] [--json]        目标、范围、下一步、版本、改动路径；不复制对话或文件正文
  handoff --done <已做> --next <下一步> [--blocker <阻碍>] [--artifact <相对路径> ...] [--json]
                                        更新当前工作区的一份小接续记录，代码版本仍由 Git 保存
  plan [--base <目标分支>] [--json]      列出待保留或可人工复核的工作区；不删除任何内容
  merge [--base main] [--check <命令> ...] [--branch <分支> ...] [--json]
                                        合并环节：把标记 ready 的分支逐个合进集成工作区，跑检查，
                                        汇总 changelog.d/ 里的条目，最后快进 main；冲突与检查失败的跳过并列出

并行规则：每项修改先 create 自己的工作区，main 只由 merge 前进；create 时范围与进行中的工作区重叠会提示。
CHANGELOG 条目写在自己分支的 changelog.d/<分支名，/ 换成 ->.md，合并时按 CHANGELOG 现有日期标题的样式汇到今天下面。
私有协作约定用 git config shifu.rules <文件> 登记；create 会在新工作区放不提交的 AGENTS.md／CLAUDE.md 指向它。
做完、提交、自测后用 handoff --ready 标记；标记后再提交会被视为未就绪，需重新标记。

接手时先用 brief 确认目录、分支、目标和范围，再读 AGENTS.md／HANDOFF.md 当前卡及需要的文件。
修改前看 git status、git diff；提交后由负责人审阅并合并。同一工作区的任务记录同时只由一位负责人更新。
跨 Agent 交接传工作区路径和 brief 入口；接收方若无权访问，明确报告缺少的路径，勿自动扩大读取权限。
源码工作区放在构建目录之外；依赖下载沿用 Cargo 的缓存，编译输出与待交付候选保持隔离。
plan 中 review 仅表示版本和文件检查通过，仍需确认没有运行中的进程并保存任务记录。";

#[derive(Default)]
struct Args {
    action: String,
    positional: Vec<String>,
    path: Option<PathBuf>,
    base: Option<String>,
    goal: Option<String>,
    scope: Vec<String>,
    done: Option<String>,
    next: Option<String>,
    blocker: Option<String>,
    artifacts: Vec<String>,
    ready: bool,
    checks: Vec<String>,
    branches: Vec<String>,
    json: bool,
}

fn parse(args: &[String]) -> Res<Args> {
    let mut parsed = Args {
        action: args.first().cloned().unwrap_or_else(|| "help".into()),
        ..Args::default()
    };
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} 缺少值"));
        match arg.as_str() {
            "--json" => parsed.json = true,
            "--path" if parsed.action == "brief" => parsed.path = Some(PathBuf::from(value()?)),
            "--ready" if parsed.action == "handoff" => parsed.ready = true,
            "--check" if parsed.action == "merge" => parsed.checks.push(value()?),
            "--branch" if parsed.action == "merge" => parsed.branches.push(value()?),
            "--base" if matches!(parsed.action.as_str(), "create" | "plan" | "merge") => {
                parsed.base = Some(value()?)
            }
            "--goal" if matches!(parsed.action.as_str(), "create" | "init") => {
                parsed.goal = Some(value()?)
            }
            "--scope" if matches!(parsed.action.as_str(), "create" | "init") => {
                parsed.scope.push(relative(&value()?)?)
            }
            "--done" if parsed.action == "handoff" => parsed.done = Some(value()?),
            "--next" if parsed.action == "handoff" => parsed.next = Some(value()?),
            "--blocker" if parsed.action == "handoff" => parsed.blocker = Some(value()?),
            "--artifact" if parsed.action == "handoff" => {
                parsed.artifacts.push(relative(&value()?)?)
            }
            _ if arg.starts_with('-') => {
                return Err(format!("{0} 不支持参数 {arg}", parsed.action));
            }
            _ => parsed.positional.push(arg.clone()),
        }
    }
    let expected = if parsed.action == "create" { 2 } else { 0 };
    if parsed.positional.len() != expected {
        return Err("运行 shifu workspace help 查看参数。".into());
    }
    Ok(parsed)
}

fn relative(raw: &str) -> Res<String> {
    if raw.is_empty()
        || Path::new(raw)
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("需要工作区内的相对路径：{raw:?}"));
    }
    Ok(raw.to_owned())
}

// Worktree creation may run checkout filters; a deadline stops this command without replaying a partial add.
fn git_output(cwd: &Path, args: &[&str]) -> Res<Output> {
    let mut child = Command::new("git")
        .args(["--no-optional-locks", "-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动 git：{e}"))?;
    let mut stdout = child.stdout.take().ok_or("git stdout 未连接")?;
    let mut stderr = child.stderr.take().ok_or("git stderr 未连接")?;
    let out = thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).map(|_| b)
    });
    let err = thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).map(|_| b)
    });
    let deadline = Instant::now()
        + Duration::from_secs(if args.starts_with(&["worktree", "add"]) {
            60
        } else {
            15
        });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => format!("等待 git 失败：{e}"),
                    _ => "git 超时；创建操作可能已部分落盘，请先用 git worktree list 核对，不自动重试。".into(),
                });
            }
        }
    };
    let stdout = out
        .join()
        .map_err(|_| "git stdout 读取中断")?
        .map_err(|e| e.to_string())?;
    let stderr = err
        .join()
        .map_err(|_| "git stderr 读取中断")?
        .map_err(|e| e.to_string())?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn git(cwd: &Path, args: &[&str]) -> Res<String> {
    let out = git_output(cwd, args)?;
    if !out.status.success() {
        return Err(format!(
            "git {}：{}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("Git 输出不是 UTF-8：{e}"))
}

fn value(cwd: &Path, args: &[&str]) -> Res<String> {
    Ok(git(cwd, args)?.trim().to_owned())
}

#[derive(Default, Serialize)]
struct Worktree {
    path: String,
    head: String,
    branch: String,
    locked: bool,
    prunable: bool,
    /// The registered goal; `None` when nobody recorded what this workspace is for.
    goal: Option<String>,
    /// Why the task note could not be read, when it exists but is unusable.
    note_error: Option<String>,
}

fn worktrees(cwd: &Path) -> Res<Vec<Worktree>> {
    let text = git(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    let mut result = Vec::new();
    let mut row = Worktree::default();
    for field in text.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            if !row.path.is_empty() {
                result.push(row);
            }
            row = Worktree {
                path: path.into(),
                ..Worktree::default()
            };
        } else if let Some(head) = field.strip_prefix("HEAD ") {
            row.head = head.into();
        } else if let Some(branch) = field.strip_prefix("branch ") {
            row.branch = branch.trim_start_matches("refs/heads/").into();
        } else if field == "locked" || field.starts_with("locked ") {
            row.locked = true;
        } else if field == "prunable" || field.starts_with("prunable ") {
            row.prunable = true;
        }
    }
    if !row.path.is_empty() {
        result.push(row);
    }
    if result.is_empty() {
        return Err("当前目录没有可用的 Git 工作区。".into());
    }
    Ok(result)
}

/// Adds what the workspace is registered for, so `list` shows which ones nobody owns on record.
fn with_goal(tree: Worktree) -> Worktree {
    if tree.prunable {
        return tree;
    }
    match load_task(Path::new(&tree.path)) {
        Ok(task) => Worktree { goal: task.map(|t| t.goal), ..tree },
        Err(error) => Worktree { note_error: Some(error), ..tree },
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Task {
    goal: String,
    scope: Vec<String>,
    base_commit: String,
    source: String,
    done: String,
    next: String,
    blocker: String,
    artifacts: Vec<String>,
    saved_head: String,
    /// Committed, self-checked and waiting for the merge step at `saved_head`.
    ready: bool,
}

fn note_path(cwd: &Path) -> Res<PathBuf> {
    Ok(PathBuf::from(value(cwd, &["rev-parse", "--absolute-git-dir"])?).join(NOTE))
}

fn load_task(cwd: &Path) -> Res<Option<Task>> {
    let path = note_path(cwd)?;
    match fs::File::open(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读 {} 失败：{e}", path.display())),
        Ok(file) => {
            let mut data = Vec::new();
            file.take((NOTE_LIMIT + 1) as u64)
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            if data.len() > NOTE_LIMIT {
                return Err("接续记录超过 16 KiB，请只保留当前状态和文件引用。".into());
            }
            serde_json::from_slice(&data)
                .map(Some)
                .map_err(|e| format!("接续记录格式错误：{e}"))
        }
    }
}

fn save_task(cwd: &Path, task: &Task) -> Res<()> {
    let text = serde_json::to_vec_pretty(task).map_err(|e| e.to_string())?;
    if text.len() > NOTE_LIMIT {
        return Err("接续记录超过 16 KiB，请只保留当前状态和文件引用。".into());
    }
    let path = note_path(cwd)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, text).map_err(|e| format!("写接续记录失败：{e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("保存接续记录失败：{e}"))
}

#[derive(Serialize)]
struct Brief {
    path: String,
    branch: String,
    head: String,
    task: Option<Task>,
    changes: Vec<String>,
    entries: Vec<String>,
}

fn brief(cwd: &Path) -> Res<Brief> {
    let path = value(cwd, &["rev-parse", "--show-toplevel"])?;
    let root = Path::new(&path);
    let entries = ["AGENTS.md", "CLAUDE.md", "HANDOFF.md", "GOALS.md"]
        .into_iter()
        .filter(|p| root.join(p).is_file())
        .map(str::to_owned)
        .collect();
    Ok(Brief {
        branch: value(root, &["branch", "--show-current"])?,
        head: value(root, &["rev-parse", "HEAD"])?,
        task: load_task(root)?,
        changes: git(
            root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
        )?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect(),
        entries,
        path,
    })
}

fn show_brief(brief: &Brief, json: bool) -> Res<()> {
    if json {
        return print_json(brief);
    }
    println!(
        "工作区：{}\n分支：{}\n当前提交：{}",
        brief.path,
        if brief.branch.is_empty() {
            "(detached)"
        } else {
            &brief.branch
        },
        brief.head
    );
    if let Some(task) = &brief.task {
        println!(
            "目标：{}\n修改范围：{}\n起点提交：{}\n已做：{}\n下一步：{}\n阻碍：{}",
            task.goal,
            task.scope.join(", "),
            task.base_commit,
            task.done,
            task.next,
            task.blocker
        );
        if task.saved_head != brief.head {
            println!("注意：接续记录之后提交已变化，请重新核对 Git 差异。");
        }
        for artifact in &task.artifacts {
            println!("产物引用：{}/{}", brief.path, artifact);
        }
        if task.ready {
            println!("已标记可合并（ready）。");
        }
    } else {
        println!("当前工作区没有 Shifu 任务记录；先从现有交接文档确定任务，不推断目标。");
        if brief.branch == "main" && !brief.changes.is_empty() {
            println!("注意：main 上有未提交改动。修改应在自己的工作区里做（shifu workspace create），main 只由 shifu workspace merge 前进。");
        }
    }
    for entry in &brief.entries {
        println!("按需读取：{}/{entry}", brief.path);
    }
    for change in &brief.changes {
        println!("改动：{change}");
    }
    println!(
        "修改前核对 git status 与 git diff；只处理已分配范围。文件正文和完整对话没有复制到本说明。"
    );
    Ok(())
}

fn required(raw: &Option<String>, name: &str) -> Res<String> {
    raw.as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .ok_or_else(|| format!("缺少 {name}"))
}

fn destination(cwd: &Path, raw: &str) -> Res<PathBuf> {
    let mut path = PathBuf::new();
    for component in cwd.join(raw).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                path.pop();
            }
            _ => path.push(component),
        }
    }
    let mut ancestor = path.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().ok_or("工作区目录不可用")?.to_owned());
        ancestor = ancestor.parent().ok_or("工作区目录不可用")?;
    }
    let mut resolved = ancestor.canonicalize().map_err(|e| e.to_string())?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    let trees = worktrees(cwd)?;
    for tree in trees {
        for output in ["target", "app/src-tauri/target"] {
            if resolved.starts_with(Path::new(&tree.path).join(output)) {
                return Err(
                    "源码工作区不能放进现有 worktree 的构建目录；请选择项目旁的独立目录。".into(),
                );
            }
        }
    }
    Ok(resolved)
}

fn create(cwd: &Path, args: &Args) -> Res<()> {
    let goal = required(&args.goal, "--goal")?;
    if args.scope.is_empty() {
        return Err("至少指定一处 --scope 修改范围。".into());
    }
    let branch = format!("shifu/{}", args.positional[0]);
    git(cwd, &["check-ref-format", "--branch", &branch])?;
    let base = value(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{}^{{commit}}", args.base.as_deref().unwrap_or("HEAD")),
        ],
    )?;
    let source = value(cwd, &["rev-parse", "--show-toplevel"])?;
    let dest = destination(cwd, &args.positional[1])?;
    if dest.exists() {
        return Err(format!("目标已存在，不覆盖：{}", dest.display()));
    }
    let task = Task {
        goal,
        scope: args.scope.clone(),
        base_commit: base.clone(),
        saved_head: base.clone(),
        source,
        ..Task::default()
    };
    if serde_json::to_vec(&task).map_err(|e| e.to_string())?.len() > NOTE_LIMIT / 2 {
        return Err("目标或范围过长，请改为简短说明和路径引用。".into());
    }
    for (branch, path, theirs) in overlaps(cwd, &base, &args.scope)? {
        eprintln!("注意：{branch} 也在改 {theirs}（{path}）。开工前先定好分工或先后，避免合并时冲突。");
    }
    let dest_str = dest.to_str().ok_or("工作区路径不是 UTF-8")?;
    git(
        cwd,
        &["worktree", "add", "-b", &branch, "--", dest_str, &base],
    )?;
    save_task(&dest, &task).map_err(|e| {
        format!(
            "工作区已建于 {}，但接续记录未保存：{e}；先检查现场，不自动重建。",
            dest.display()
        )
    })?;
    point_to_rules(&dest)?;
    show_brief(&brief(&dest)?, args.json)
}

/// A public repository cannot carry private agent rules, so `git config shifu.rules <file>` names them and every new
/// workspace gets untracked AGENTS.md/CLAUDE.md pointers (kept out of commits through the shared info/exclude).
fn point_to_rules(dest: &Path) -> Res<()> {
    let Ok(rules) = value(dest, &["config", "--get", "shifu.rules"]) else { return Ok(()) };
    if rules.is_empty() {
        return Ok(());
    }
    let common = PathBuf::from(value(dest, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let exclude = common.join("info/exclude");
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    let missing: Vec<&str> = ["/AGENTS.md", "/CLAUDE.md"].into_iter().filter(|l| !current.lines().any(|c| c == *l)).collect();
    if !missing.is_empty() {
        fs::create_dir_all(common.join("info")).map_err(|e| e.to_string())?;
        let sep = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
        fs::write(&exclude, format!("{current}{sep}{}\n", missing.join("\n"))).map_err(|e| format!("写 info/exclude 失败：{e}"))?;
    }
    for (name, body) in [
        ("AGENTS.md", format!("先读并遵守 {rules}（本项目不随 Git 提交的协作约定）。\n")),
        ("CLAUDE.md", format!("@{rules}\n")),
    ] {
        let path = dest.join(name);
        if !path.exists() {
            fs::write(&path, body).map_err(|e| format!("写 {name} 失败：{e}"))?;
        }
    }
    Ok(())
}

fn handoff(cwd: &Path, args: &Args) -> Res<()> {
    let mut task = load_task(cwd)?.ok_or(
        "当前工作区没有任务记录，请先用 workspace init 或 workspace create 登记明确的任务。",
    )?;
    task.done = required(&args.done, "--done")?;
    task.next = required(&args.next, "--next")?;
    task.blocker = args.blocker.clone().unwrap_or_default();
    let root = PathBuf::from(value(cwd, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    for path in &args.artifacts {
        let full = root
            .join(path)
            .canonicalize()
            .map_err(|e| format!("产物引用不可用 {path}：{e}"))?;
        if !full.starts_with(&root) {
            return Err(format!("产物引用超出工作区：{path}"));
        }
    }
    task.artifacts = args.artifacts.clone();
    task.saved_head = value(cwd, &["rev-parse", "HEAD"])?;
    task.ready = args.ready;
    save_task(cwd, &task)?;
    show_brief(&brief(cwd)?, args.json)
}

#[derive(Serialize)]
struct Plan {
    path: String,
    branch: String,
    disposition: &'static str,
    reasons: Vec<&'static str>,
    changes: Vec<String>,
    ignored: Vec<String>,
    error: Option<String>,
}

fn plan(cwd: &Path, args: &Args) -> Res<()> {
    let trees = worktrees(cwd)?;
    let base = value(
        cwd,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!(
                "{}^{{commit}}",
                args.base.as_deref().unwrap_or(&trees[0].head)
            ),
        ],
    )?;
    let current = value(cwd, &["rev-parse", "--show-toplevel"])?;
    let mut result = Vec::new();
    for (i, tree) in trees.iter().enumerate() {
        let mut row = Plan {
            path: tree.path.clone(),
            branch: tree.branch.clone(),
            disposition: "keep",
            reasons: Vec::new(),
            changes: Vec::new(),
            ignored: Vec::new(),
            error: None,
        };
        if i == 0 {
            row.reasons.push("primary");
        }
        if tree.path == current {
            row.reasons.push("current");
        }
        if tree.locked {
            row.reasons.push("locked");
        }
        if tree.prunable || !Path::new(&tree.path).is_dir() {
            row.reasons.push("unavailable");
        } else {
            match git(
                Path::new(&tree.path),
                &[
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--ignored=matching",
                    "--untracked-files=normal",
                ],
            ) {
                Ok(status) => {
                    for field in status.split('\0').filter(|s| !s.is_empty()) {
                        if let Some(path) = field.strip_prefix("!! ") {
                            row.ignored.push(path.to_owned());
                        } else {
                            row.changes.push(field.to_owned());
                        }
                    }
                }
                Err(e) => {
                    row.error = Some(e);
                    row.reasons.push("unverifiable");
                }
            }
            if !row.changes.is_empty() {
                row.reasons.push("changes");
            }
            if !row.ignored.is_empty() {
                row.reasons.push("ignored-files");
            }
            let merged = git_output(cwd, &["merge-base", "--is-ancestor", &tree.head, &base])?;
            match merged.status.code() {
                Some(0) => {}
                Some(1) => row.reasons.push("unmerged"),
                _ => {
                    row.reasons.push("unverifiable");
                    row.error = Some(String::from_utf8_lossy(&merged.stderr).trim().to_owned());
                }
            }
        }
        if row.reasons.is_empty() {
            row.disposition = "review";
        }
        result.push(row);
    }
    if args.json {
        return print_json(&result);
    }
    println!(
        "只读回收清单；review 仍需确认进程已结束、任务记录已保存，不代表可直接删除。目标提交：{base}"
    );
    for row in &result {
        println!(
            "{}\t{}\t{}\t{}",
            row.disposition,
            row.branch,
            row.path,
            row.reasons.join(",")
        );
    }
    Ok(())
}

fn print_json(value: &impl Serialize) -> Res<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn init(cwd: &Path, args: &Args) -> Res<()> {
    let goal = required(&args.goal, "--goal")?;
    if args.scope.is_empty() {
        return Err("至少指定一处 --scope 修改范围。".into());
    }
    if load_task(cwd)?.is_some() {
        return Err("当前工作区已有任务记录；用 brief 核对、handoff 更新，不覆盖原目标。".into());
    }
    let head = value(cwd, &["rev-parse", "HEAD"])?;
    let task = Task {
        goal,
        scope: args.scope.clone(),
        base_commit: head.clone(),
        saved_head: head,
        source: value(cwd, &["rev-parse", "--show-toplevel"])?,
        ..Task::default()
    };
    save_task(cwd, &task)?;
    show_brief(&brief(cwd)?, args.json)
}

fn is_ancestor(cwd: &Path, commit: &str, of: &str) -> Res<bool> {
    let out = git_output(cwd, &["merge-base", "--is-ancestor", commit, of])?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()),
    }
}

/// Other workspaces still working (task note, not yet merged) whose scope overlaps `scope`.
fn overlaps(cwd: &Path, base: &str, scope: &[String]) -> Res<Vec<(String, String, String)>> {
    let mut found = Vec::new();
    for tree in worktrees(cwd)? {
        if tree.prunable || !Path::new(&tree.path).is_dir() {
            continue;
        }
        let Ok(Some(task)) = load_task(Path::new(&tree.path)) else { continue };
        // Merged work is finished; a fresh workspace (no commits yet) is still active.
        if tree.head != task.base_commit && is_ancestor(cwd, &tree.head, base).unwrap_or(false) {
            continue;
        }
        for theirs in &task.scope {
            if scope.iter().any(|mine| Path::new(mine).starts_with(theirs) || Path::new(theirs).starts_with(mine)) {
                found.push((tree.branch.clone(), tree.path.clone(), theirs.clone()));
            }
        }
    }
    Ok(found)
}

fn today(format: &str) -> Res<String> {
    let out = Command::new("date").arg(format).output().map_err(|e| format!("无法取日期：{e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Where a heading carries its date: a full YYYY-MM-DD, or else a bare MM-DD. Returns the byte range and whether it is full.
fn date_in(line: &str) -> Option<(usize, usize, bool)> {
    let b = line.as_bytes();
    let digits = |at: usize, n: usize| at + n <= b.len() && b[at..at + n].iter().all(u8::is_ascii_digit);
    let edge = |at: usize| at >= b.len() || !b[at].is_ascii_digit();
    let starts = |at: usize| at == 0 || !b[at - 1].is_ascii_digit();
    for at in 0..b.len() {
        if starts(at) && digits(at, 4) && b.get(at + 4) == Some(&b'-') && digits(at + 5, 2) && b.get(at + 7) == Some(&b'-') && digits(at + 8, 2) && edge(at + 10) {
            return Some((at, at + 10, true));
        }
    }
    (0..b.len()).find(|&at| starts(at) && digits(at, 2) && b.get(at + 2) == Some(&b'-') && digits(at + 3, 2) && edge(at + 5)).map(|at| (at, at + 5, false))
}

/// Puts `text` under today's heading, reusing the style of the newest dated `## ` heading (or `## MM-DD` if none).
fn insert_today(log: &str, text: &str) -> Res<String> {
    let mut offset = 0;
    for line in log.split_inclusive('\n') {
        if let (true, Some((a, b, full))) = (line.starts_with("## "), date_in(line)) {
            let day = today(if full { "+%Y-%m-%d" } else { "+%m-%d" })?;
            let after = offset + line.len();
            if line[a..b] == day {
                return Ok(format!("{}\n{text}{}", &log[..after], &log[after..]));
            }
            let heading = format!("{}{day}{}", &line[..a], line[b..].trim_end());
            return Ok(format!("{}{heading}\n\n{text}\n{}", &log[..offset], &log[offset..]));
        }
        offset += line.len();
    }
    Ok(format!("{}\n\n## {}\n\n{text}", log.trim_end(), today("+%m-%d")?))
}

/// Moves every changelog.d/*.md note under today's heading in CHANGELOG.md and commits. Returns whether it did.
fn fold_changelog(root: &Path) -> Res<bool> {
    let dir = root.join("changelog.d");
    let Ok(entries) = fs::read_dir(&dir) else { return Ok(false) };
    let mut notes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.file_name().is_some_and(|n| n != "README.md"))
        .collect();
    if notes.is_empty() {
        return Ok(false);
    }
    notes.sort();
    let mut text = String::new();
    for note in &notes {
        let body = fs::read_to_string(note).map_err(|e| format!("读 {} 失败：{e}", note.display()))?;
        if !body.trim().is_empty() {
            text.push_str(body.trim());
            text.push('\n');
        }
    }
    let path = root.join("CHANGELOG.md");
    let log = fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() {
        let updated = insert_today(&log, &text)?;
        fs::write(&path, updated).map_err(|e| format!("写 CHANGELOG 失败：{e}"))?;
        git(root, &["add", "--", "CHANGELOG.md"])?;
    }
    let mut rm = vec!["rm", "-q", "--"];
    let names: Vec<String> = notes.iter().map(|n| n.strip_prefix(root).unwrap_or(n).to_string_lossy().into_owned()).collect();
    rm.extend(names.iter().map(String::as_str));
    git(root, &rm)?;
    git(root, &["commit", "-q", "--no-verify", "-m", "docs: changelog"])?;
    Ok(true)
}

#[derive(Default, Serialize)]
struct Skipped {
    branch: String,
    files: Vec<String>,
    detail: String,
}

#[derive(Default, Serialize)]
struct MergeReport {
    base: String,
    integration: String,
    merged: Vec<String>,
    conflicts: Vec<Skipped>,
    check_failed: Vec<Skipped>,
    changed_after_ready: Vec<Skipped>,
    head: String,
    main_updated: bool,
    main_error: Option<String>,
}

/// The merge step. All merging happens in a dedicated integration worktree, so nobody's checkout is
/// touched until the end, when the base branch only moves forward (fast-forward) if Git allows it.
fn merge(cwd: &Path, args: &Args) -> Res<()> {
    let trees = worktrees(cwd)?;
    let base_name = args.base.clone().unwrap_or_else(|| "main".into());
    let start = value(cwd, &["rev-parse", "--verify", "--end-of-options", &format!("{base_name}^{{commit}}")])?;
    let mut report = MergeReport { base: base_name.clone(), ..Default::default() };

    let mut candidates = Vec::new();
    if args.branches.is_empty() {
        for tree in trees.iter().filter(|t| !t.branch.is_empty() && t.branch != base_name) {
            if tree.prunable || !Path::new(&tree.path).is_dir() {
                continue;
            }
            let Ok(Some(task)) = load_task(Path::new(&tree.path)) else { continue };
            if !task.ready || is_ancestor(cwd, &tree.head, &start)? {
                continue;
            }
            if tree.head != task.saved_head {
                report.changed_after_ready.push(Skipped {
                    branch: tree.branch.clone(),
                    detail: "标记 ready 后又有提交，请自测后重新 handoff --ready".into(),
                    ..Default::default()
                });
                continue;
            }
            candidates.push(tree.branch.clone());
        }
    } else {
        for branch in &args.branches {
            let head = value(cwd, &["rev-parse", "--verify", "--end-of-options", &format!("refs/heads/{branch}^{{commit}}")])?;
            if !is_ancestor(cwd, &head, &start)? {
                candidates.push(branch.clone());
            }
        }
    }

    let primary = Path::new(&trees[0].path);
    let name = primary.file_name().ok_or("主工作区路径不可用")?.to_string_lossy();
    let integ = primary.parent().ok_or("主工作区路径不可用")?.join(format!("{name}-integrate-wt"));
    let integ_str = integ.to_str().ok_or("集成工作区路径不是 UTF-8")?.to_owned();
    report.integration = integ_str.clone();
    if trees.iter().any(|t| Path::new(&t.path) == integ) {
        if !git(&integ, &["status", "--porcelain"])?.trim().is_empty() {
            return Err(format!("集成工作区有未提交内容，先处理再合并：{integ_str}"));
        }
        git(&integ, &["checkout", "-q", "--detach", &start])?;
    } else if integ.exists() {
        return Err(format!("集成工作区位置已被占用且不是本仓库的工作区：{integ_str}"));
    } else {
        git(cwd, &["worktree", "add", "-q", "--detach", "--", &integ_str, &start])?;
    }

    for branch in candidates {
        let before = value(&integ, &["rev-parse", "HEAD"])?;
        let merged = git_output(&integ, &["merge", "--no-ff", "--no-edit", "-m", &format!("merge {branch}"), &branch])?;
        if !merged.status.success() {
            let files = git(&integ, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
            let _ = git(&integ, &["merge", "--abort"]);
            git(&integ, &["reset", "-q", "--hard", &before])?;
            report.conflicts.push(Skipped {
                branch,
                files: files.lines().map(str::to_owned).collect(),
                detail: String::from_utf8_lossy(&merged.stdout).lines().last().unwrap_or("").to_owned(),
            });
            continue;
        }
        fold_changelog(&integ)?;
        let mut failed = None;
        for check in &args.checks {
            let out = Command::new("sh").args(["-c", check]).current_dir(&integ).stdin(Stdio::null()).output()
                .map_err(|e| format!("无法运行检查 {check}：{e}"))?;
            if !out.status.success() {
                let tail: String = String::from_utf8_lossy(&[out.stdout, out.stderr].concat()).lines().rev().take(8)
                    .collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                failed = Some(format!("{check}\n{tail}"));
                break;
            }
        }
        if let Some(detail) = failed {
            git(&integ, &["reset", "-q", "--hard", &before])?;
            report.check_failed.push(Skipped { branch, detail, ..Default::default() });
            continue;
        }
        report.merged.push(branch);
    }

    report.head = value(&integ, &["rev-parse", "HEAD"])?;
    if report.head != start {
        let result = match trees.iter().find(|t| t.branch == base_name) {
            Some(tree) => git(Path::new(&tree.path), &["merge", "--ff-only", "-q", &report.head]),
            None => git(cwd, &["update-ref", &format!("refs/heads/{base_name}"), &report.head, &start]),
        };
        match result {
            Ok(_) => report.main_updated = true,
            Err(e) => report.main_error = Some(format!("合并结果在集成工作区提交 {}，{base_name} 未前进：{e}", report.head)),
        }
    }

    if args.json {
        return print_json(&report);
    }
    println!("合并到 {base_name}（集成工作区 {}）", report.integration);
    for b in &report.merged {
        println!("已合并\t{b}");
    }
    for s in &report.conflicts {
        println!("冲突，跳过\t{}\t{}", s.branch, s.files.join(", "));
    }
    for s in &report.check_failed {
        println!("检查未过，跳过\t{}\n{}", s.branch, s.detail);
    }
    for s in &report.changed_after_ready {
        println!("标记后有新提交，跳过\t{}", s.branch);
    }
    match (&report.main_error, report.main_updated) {
        (Some(e), _) => println!("{e}"),
        (None, true) => println!("{base_name} 已前进到 {}", report.head),
        (None, false) => println!("没有可合并的分支，{base_name} 未变"),
    }
    Ok(())
}

pub fn run(args: &[String]) -> Res<()> {
    let args = parse(args)?;
    if matches!(args.action.as_str(), "help" | "guide" | "-h" | "--help") {
        println!("{GUIDE}");
        return Ok(());
    }
    let cwd = args
        .path
        .clone()
        .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
    match args.action.as_str() {
        "list" => {
            let trees: Vec<Worktree> = worktrees(&cwd)?.into_iter().map(with_goal).collect();
            if args.json {
                print_json(&trees)
            } else {
                for tree in &trees {
                    let note = match (&tree.goal, &tree.note_error) {
                        (Some(goal), _) => goal.clone(),
                        (None, Some(error)) => format!("（记录读不出：{error}）"),
                        (None, None) => "（没有任务记录）".into(),
                    };
                    println!("{}\t{}\t{}\t{}", tree.branch, tree.head, tree.path, note);
                }
                let bare = trees.iter().filter(|t| t.goal.is_none()).count();
                if bare > 0 {
                    println!("{bare} 个工作区没有任务记录：在该目录运行 shifu workspace init --goal … --scope … 补登记。");
                }
                Ok(())
            }
        }
        "create" => create(&cwd, &args),
        "init" => init(&cwd, &args),
        "brief" => show_brief(&brief(&cwd)?, args.json),
        "handoff" => handoff(&cwd, &args),
        "plan" => plan(&cwd, &args),
        "merge" => merge(&cwd, &args),
        _ => Err("运行 shifu workspace help 查看命令。".into()),
    }
}
