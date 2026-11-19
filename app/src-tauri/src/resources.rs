//! How much memory each agent takes, and how full the machine is (用户 11-10: "像 Orca 一样" count windows and
//! memory). An agent's memory is the footprint Activity Monitor shows ("内存"), summed over its pane's process tree
//! (Claude starts MCP servers and shells under it). The machine's figures are the kernel's free-memory percentage,
//! the number Orca's `diagnostics memory` reports as host used.
//!
//! After Orca's resource manager (11-10): CPU next to memory, Shifu itself split into the app, its web page
//! (WebKit runs the page in separate processes launchd starts, so they are not our children) and tmux, and
//! terminals in our tmux that no agent owns, which would otherwise take memory nobody sees.

use serde::Serialize;
use shifu::agent;
use shifu::status::Status;
use shifu::tmux::Tmux;
use std::collections::HashMap;
use std::process::Command;

#[derive(Serialize, Debug, PartialEq)]
pub struct AgentMem {
    pub name: String,
    pub bytes: u64,
    /// Processes in its tree (the agent itself, MCP servers, shells it runs).
    pub processes: usize,
    /// `ps` %CPU summed over the tree: 100 is one core busy.
    pub cpu: f32,
}

#[derive(Serialize, Debug, Default)]
pub struct ShifuMem {
    pub app: u64,
    /// WebKit's page, GPU and network processes working for this app.
    pub web: u64,
    pub tmux: u64,
    pub cpu: f32,
}

/// A tmux pane no agent record points at.
#[derive(Serialize, Debug, PartialEq)]
pub struct Stray {
    pub session: String,
    pub bytes: u64,
    pub cpu: f32,
}

#[derive(Serialize, Debug)]
pub struct Resources {
    /// Live agents, biggest first.
    pub agents: Vec<AgentMem>,
    pub shifu: ShifuMem,
    pub strays: Vec<Stray>,
    pub host_total: u64,
    /// Percent of memory in use (100 minus the kernel's free percentage).
    pub host_used_percent: u32,
    /// 1 normal, 2 warning, 4 critical (`kern.memorystatus_vm_pressure_level`).
    pub pressure: u32,
}

/// `pid -> children` and `pid -> %CPU` from `ps -axo pid=,ppid=,pcpu=`.
pub fn parse_ps(ps: &str) -> (HashMap<i32, Vec<i32>>, HashMap<i32, f32>) {
    let mut kids: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut cpu = HashMap::new();
    for line in ps.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(ppid)) = (it.next().and_then(|w| w.parse().ok()), it.next().and_then(|w| w.parse().ok())) else {
            continue;
        };
        kids.entry(ppid).or_default().push(pid);
        if let Some(c) = it.next().and_then(|w| w.parse::<f32>().ok()) {
            cpu.insert(pid, c);
        }
    }
    (kids, cpu)
}

/// `root` and everything under it.
pub fn tree(root: i32, kids: &HashMap<i32, Vec<i32>>) -> Vec<i32> {
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        if let Some(k) = kids.get(&all[i]) {
            all.extend(k);
        }
        i += 1;
    }
    all
}

/// Activity Monitor's "Memory" for one process; 0 when it is gone or not ours to read.
fn footprint(pid: i32) -> u64 {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a rusage_info_v2, the struct the RUSAGE_INFO_V2 flavor fills.
    let rc = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, &mut info as *mut _ as *mut libc::rusage_info_t) };
    if rc == 0 { info.ri_phys_footprint } else { 0 }
}

unsafe extern "C" {
    /// libsystem's answer to "which app is this process working for" (what Activity Monitor uses to file
    /// WebKit's helpers under the app). Not in a public header, exported since macOS 10.14.
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

/// WebKit's helper processes working for `owner`.
fn web_helpers(owner: i32, cpu: &HashMap<i32, f32>) -> Vec<i32> {
    let ps = Command::new("pgrep").args(["-f", "com.apple.WebKit"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    ps.unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse::<i32>().ok())
        // SAFETY: takes a pid and returns one; no memory is shared.
        .filter(|&p| cpu.contains_key(&p) && unsafe { responsibility_get_pid_responsible_for_pid(p) } == owner)
        .collect()
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    // SAFETY: buf is 8 bytes and `len` says so; the kernel writes at most `len` bytes and updates it.
    let rc = unsafe { libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    match (rc, len) {
        (0, 4) => Some(u32::from_ne_bytes(buf[..4].try_into().ok()?) as u64),
        (0, 8) => Some(u64::from_ne_bytes(buf)),
        _ => None,
    }
}

/// `(pane_id, pane_pid, session)` from `list-panes -a -F '#{pane_id} #{pane_pid} #{session_name}'`.
pub fn parse_panes(out: &str) -> Vec<(String, i32, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            Some((it.next()?.to_string(), it.next()?.parse().ok()?, it.next()?.to_string()))
        })
        .collect()
}

fn sum(pids: &[i32], cpu: &HashMap<i32, f32>) -> (u64, f32) {
    (pids.iter().map(|&p| footprint(p)).sum(), pids.iter().filter_map(|p| cpu.get(p)).sum())
}

pub fn collect() -> Resources {
    let ps = Command::new("ps").args(["-axo", "pid=,ppid=,pcpu="]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let (kids, cpu) = parse_ps(&ps.unwrap_or_default());
    let tmux = Tmux::from_env();
    let tmux_panes = parse_panes(&tmux.run(["list-panes", "-a", "-F", "#{pane_id} #{pane_pid} #{session_name}"]).unwrap_or_default());
    let pane_pid = |id: &str| tmux_panes.iter().find(|(p, _, _)| p == id).map(|(_, pid, _)| *pid);
    let panes = tmux.panes();
    let (records, _) = agent::list();
    let mut agents: Vec<AgentMem> = records
        .iter()
        .filter(|a| a.effective_status(&panes) != Status::Exited)
        .filter_map(|a| {
            let pids = tree(pane_pid(a.pane_id.as_deref()?)?, &kids);
            let (bytes, cpu) = sum(&pids, &cpu);
            Some(AgentMem { name: a.name.clone(), bytes, processes: pids.len(), cpu })
        })
        .collect();
    agents.sort_by_key(|a| std::cmp::Reverse(a.bytes));
    let owned: Vec<&str> = records.iter().filter_map(|a| a.pane_id.as_deref()).collect();
    let mut strays: Vec<Stray> = tmux_panes
        .iter()
        .filter(|(id, _, _)| !owned.contains(&id.as_str()))
        .map(|(_, pid, session)| {
            let (bytes, cpu) = sum(&tree(*pid, &kids), &cpu);
            Stray { session: session.clone(), bytes, cpu }
        })
        .collect();
    strays.sort_by_key(|s| std::cmp::Reverse(s.bytes));
    let me = std::process::id() as i32;
    let server = tmux.run(["display-message", "-p", "#{pid}"]).ok().and_then(|p| p.trim().parse::<i32>().ok());
    let (app, app_cpu) = sum(&[me], &cpu);
    let (web, web_cpu) = sum(&web_helpers(me, &cpu), &cpu);
    let (tmux_bytes, tmux_cpu) = sum(server.as_slice(), &cpu);
    let shifu = ShifuMem { app, web, tmux: tmux_bytes, cpu: app_cpu + web_cpu + tmux_cpu };
    let free = sysctl_u64("kern.memorystatus_level").unwrap_or(100).min(100) as u32;
    Resources {
        agents,
        shifu,
        strays,
        host_total: sysctl_u64("hw.memsize").unwrap_or(0),
        host_used_percent: 100 - free,
        pressure: sysctl_u64("kern.memorystatus_vm_pressure_level").unwrap_or(1) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_the_root_and_everything_under_it() {
        let (kids, cpu) = parse_ps("  1     0   0.0\n 10     1  12.5\n 11    10   0.3\n 12    10   0.0\n 13    11 101.0\n 20     1   0.0\nbad line\n");
        let mut t = tree(10, &kids);
        t.sort();
        assert_eq!(t, [10, 11, 12, 13]);
        assert_eq!(tree(99, &kids), [99]);
        let total: f32 = t.iter().filter_map(|p| cpu.get(p)).sum();
        assert!((total - 113.8).abs() < 0.01, "{total}");
    }

    #[test]
    fn panes_keep_session_names_with_spaces() {
        assert_eq!(
            parse_panes("%1 501 claude-Workspace\n%7 900 my odd session\nbroken\n"),
            [("%1".into(), 501, "claude-Workspace".into()), ("%7".into(), 900, "my odd session".into())]
        );
    }

    #[test]
    fn this_app_owns_no_webkit_helpers_but_the_lookup_answers() {
        // SAFETY: as in web_helpers.
        assert_eq!(unsafe { responsibility_get_pid_responsible_for_pid(std::process::id() as i32) }, std::process::id() as i32);
        assert!(web_helpers(std::process::id() as i32, &HashMap::new()).is_empty());
    }

    #[test]
    fn this_process_has_a_footprint_and_the_machine_has_memory() {
        assert!(footprint(std::process::id() as i32) > 1_000_000);
        assert_eq!(footprint(-1), 0);
        assert!(sysctl_u64("hw.memsize").unwrap() > 1 << 30);
        assert!(sysctl_u64("kern.memorystatus_level").unwrap() <= 100);
    }
}
