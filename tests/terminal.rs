//! Exercise the same session backend as the desktop in an isolated tmux server.
use shifu::{agent, app_session, status::Status, tmux::Tmux};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "terminal did not become ready");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn terminal_opens_in_project_accepts_input_and_restarts() {
    if std::env::var_os("SHIFU_TERMINAL_TEST_CHILD").is_none() {
        let dir = std::env::temp_dir().join(format!("shifu-terminal-{}", std::process::id()));
        fs::create_dir_all(dir.join("project folder")).unwrap();
        let shell = dir.join("login shell's wrapper");
        fs::write(&shell, "#!/bin/sh\nif [ \"$1\" = -l ]; then printf '%s\\n' \"$@\" >> \"$SHIFU_HOME/shell-args\"; fi\nexec /bin/sh \"$@\"\n").unwrap();
        fs::set_permissions(&shell, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = format!("shifu-terminal-test-{}", std::process::id());
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_opens_in_project_accepts_input_and_restarts",
                "--nocapture",
            ])
            .env("SHIFU_TERMINAL_TEST_CHILD", "1")
            .env("SHIFU_HOME", &dir)
            .env("SHIFU_SOCKET", &socket)
            .env("SHELL", &shell)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .output()
            .unwrap();
        let _ = Command::new(shifu::tmux::find_tmux_bin())
            .args(["-L", &socket, "kill-server"])
            .output();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let home = agent::shifu_home();
    let cwd = home.join("project folder").canonicalize().unwrap();
    let cfg = app_session::Config {
        home: home.clone(),
        socket: std::env::var("SHIFU_SOCKET").unwrap(),
        shifu_bin: env!("CARGO_BIN_EXE_shifu").into(),
        user_statusline: None,
    };
    let name =
        app_session::create_agent(&cfg, "terminal", cwd.to_str().unwrap(), None, None).unwrap();
    let record = agent::load(&name).unwrap().unwrap();
    assert_eq!(record.kind, agent::Kind::Other);
    assert_eq!(record.label.as_deref(), Some("终端"));
    assert!(record.model.is_none() && record.effort.is_none());
    let tmux = Tmux::from_env();
    let pane = record.pane_id.as_deref().unwrap();
    wait_for(|| fs::read_to_string(home.join("shell-args")).is_ok_and(|s| s == "-l\n"));
    assert_eq!(record.effective_status(&tmux.panes()), Status::Running);
    let output = home.join("pwd-result");
    tmux.run([
        "send-keys",
        "-t",
        pane,
        "-l",
        &format!(
            "pwd > {}",
            shifu::launch::sh_quote(output.to_str().unwrap())
        ),
    ])
    .unwrap();
    tmux.run(["send-keys", "-t", pane, "Enter"]).unwrap();
    wait_for(|| output.exists());
    assert_eq!(
        fs::read_to_string(&output).unwrap().trim(),
        cwd.to_str().unwrap()
    );
    tmux.run(["send-keys", "-t", pane, "exit", "Enter"])
        .unwrap();
    wait_for(|| record.effective_status(&tmux.panes()) == Status::Exited);
    app_session::resume_agent(&cfg, &name).unwrap();
    wait_for(|| fs::read_to_string(home.join("shell-args")).is_ok_and(|s| s == "-l\n-l\n"));
    assert_eq!(
        agent::load(&name)
            .unwrap()
            .unwrap()
            .effective_status(&tmux.panes()),
        Status::Running
    );
    let second =
        app_session::create_agent(&cfg, "terminal", cwd.to_str().unwrap(), None, None).unwrap();
    assert_ne!(name, second);
}
