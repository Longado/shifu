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

  list [--json]                         列出当前仓库所有 worktree
  init --goal <目标> --scope <相对路径> [--scope ...] [--json]
                                        在现有工作区登记任务，不另建工作区，不覆盖已有记录
  create <名字> <目录> --goal <目标> --scope <相对路径> [--scope ...] [--base <提交>] [--json]
                                        从已提交版本建 shifu/<名字> 分支；不复制缓存或未提交文件
  brief [--path <工作区>] [--json]        目标、范围、下一步、版本、改动路径；不复制对话或文件正文
  handoff --done <已做> --next <下一步> [--blocker <阻碍>] [--artifact <相对路径> ...] [--json]
                                        更新当前工作区的一份小接续记录，代码版本仍由 Git 保存
  plan [--base <目标分支>] [--json]      列出待保留或可人工复核的工作区；不删除任何内容

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
            "--base" if matches!(parsed.action.as_str(), "create" | "plan") => {
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

#[derive(Default, Serialize, Deserialize)]
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
    } else {
        println!("当前工作区没有 Shifu 任务记录；先从现有交接文档确定任务，不推断目标。");
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
    show_brief(&brief(&dest)?, args.json)
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
            let trees = worktrees(&cwd)?;
            if args.json {
                print_json(&trees)
            } else {
                for tree in trees {
                    println!("{}\t{}\t{}", tree.branch, tree.head, tree.path);
                }
                Ok(())
            }
        }
        "create" => create(&cwd, &args),
        "init" => init(&cwd, &args),
        "brief" => show_brief(&brief(&cwd)?, args.json),
        "handoff" => handoff(&cwd, &args),
        "plan" => plan(&cwd, &args),
        _ => Err("运行 shifu workspace help 查看命令。".into()),
    }
}
