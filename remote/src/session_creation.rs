//! Create registered desktop sessions through an approved device's encrypted channel.
use serde::{Deserialize, Serialize};
use shifu::{agent, app_session, app_statusline, tmux::Tmux};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CREATION: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
pub(crate) struct Project {
    path: String,
    name: String,
}

#[derive(Serialize)]
pub(crate) struct AgentChoice {
    kind: &'static str,
    name: &'static str,
    installed: bool,
}

#[derive(Serialize)]
pub(crate) struct Catalog {
    projects: Vec<Project>,
    agents: Vec<AgentChoice>,
}

#[derive(Deserialize, Default)]
struct Tasks {
    #[serde(default)]
    tasks: Vec<TaskProject>,
}
#[derive(Deserialize)]
struct TaskProject {
    project: String,
}

fn projects(home: &Path, agent_dirs: impl Iterator<Item = String>) -> super::Result<Vec<Project>> {
    let tasks: Tasks = match std::fs::read(home.join("tasks.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("远端项目记录无法读取：{e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Tasks::default(),
        Err(e) => return Err(format!("远端项目记录无法读取：{e}")),
    };
    let paths: BTreeSet<_> = tasks.tasks.into_iter().map(|t| t.project).chain(agent_dirs)
        .filter(|path| Path::new(path).is_absolute())
        .filter_map(|path| Path::new(&path).canonicalize().ok())
        .filter(|path| path.is_dir())
        .collect();
    Ok(paths.into_iter().map(|path| Project {
        name: path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
    }).collect())
}

fn installed(command: &str) -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).any(|dir| {
        std::fs::metadata(dir.join(command)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

pub(crate) fn catalog() -> super::Result<Catalog> {
    let (agents, _) = agent::list();
    Ok(Catalog {
        projects: projects(&agent::shifu_home(), agents.into_iter().map(|a| a.cwd))?,
        agents: [("claude", "Claude"), ("codex", "Codex")].into_iter().map(|(kind, name)| {
            AgentChoice {kind, name, installed: installed(kind)}
        }).collect(),
    })
}

pub(crate) fn create(project: &str, kind: &str) -> super::Result<String> {
    let _held = CREATION.lock().map_err(|_| "远端新建服务需要重新启动")?;
    let values = catalog()?;
    if !values.projects.iter().any(|p| p.path == project) {
        return Err("请选择远端已登记且仍存在的项目".into());
    }
    if !values.agents.iter().any(|a| a.kind == kind && a.installed) {
        return Err("远端未安装所选Agent，请先完成安装".into());
    }
    let user_home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config = app_session::Config {
        home: agent::shifu_home(),
        socket: Tmux::from_env().socket,
        shifu_bin: std::env::var("SHIFU_APP_SHIFU").ok().filter(|v| !v.is_empty())
            .unwrap_or_else(|| user_home.join(".cargo/bin/shifu").to_string_lossy().into_owned()),
        user_statusline: app_statusline::user_command_from_file(&user_home.join(".claude/settings.json")),
    };
    app_session::create_agent(&config, kind, project, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_uses_registered_existing_directories_and_preserves_corrupt_file_errors() {
        let home = std::env::temp_dir().join(format!("shifu-projects-{}", std::process::id()));
        std::fs::create_dir_all(home.join("中文 project")).unwrap();
        let project = home.join("中文 project").canonicalize().unwrap().to_string_lossy().into_owned();
        std::fs::write(home.join("tasks.json"), serde_json::json!({"tasks":[
            {"project":project}, {"project":home.join("missing")}, {"project":"relative"}
        ]}).to_string()).unwrap();
        let result = projects(&home, vec![project.clone()].into_iter()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, project);
        assert_eq!(result[0].name, "中文 project");
        std::fs::write(home.join("tasks.json"), "broken").unwrap();
        assert!(projects(&home, std::iter::empty()).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}
