//! The real CLI, disk queue, tmux adapter and hook acknowledgement, with only the terminal boundary isolated.
use shifu::{
    agent::{Agent, Kind},
    inbox::{self, Phase},
    status::Status,
    tmux::Tmux,
};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn queued_native_input_preserves_drafts_and_never_retries_an_ambiguous_send() {
    let root = std::env::temp_dir().join(format!("shifu-inbox-{}", std::process::id()));
    fs::create_dir_all(root.join("home/agents")).unwrap();
    fs::create_dir_all(root.join("codex/sessions")).unwrap();
    let native_log = root.join("codex/sessions/rollout-native-demo.jsonl");
    fs::write(
        &native_log,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"native-demo\"}}\n",
    )
    .unwrap();
    let home = root.join("home");
    let fake = root.join("tmux");
    fs::write(
        &fake,
        r##"#!/usr/bin/env python3
import os, pathlib, sys
p=pathlib.Path(os.environ['INBOX_FIXTURE']); a=sys.argv[5:]
if a[0]=='list-panes':
 def row(pane, window, title):
  fields=f'{pane} 0 {window} 0'
  if 'pane_title' not in a[-1]: return fields
  return fields+'\t'+title if 'pane_dead' in a[-1] else pane+' '+title
 print(row('%1','@1','Task'))
 if (p/'both-ready').exists(): print(row('%2','@2','Second'))
elif a[0]=='capture-pane': print('› keep my draft' if a[-1]=='%2' and not (p/'both-ready').exists() else (p/'screen').read_text())
elif a[0]=='set-buffer': (p/'buffer').write_text(a[a.index('--')+1])
elif a[0]=='paste-buffer':
 with (p/'pastes').open('a') as f: f.write((p/'buffer').read_text()+'\n')
 text=(p/'buffer').read_text()
 if (p/'edit-after-paste').exists(): text=text.replace('blue file','bluefile')
 (p/'screen').write_text('Working\n› '+text+'\nGPT-6')
 if (p/'stop-on-paste').exists(): (p/'STOP').touch()
elif a[0]=='send-keys':
 with (p/'enters').open('a') as f: f.write('Enter\n')
 if (p/'fail').exists(): sys.exit(1)
 (p/'screen').write_text('Working\n› Ask Codex to do anything\nGPT-6')
"##,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let agent = Agent {
        name: "codex-demo".into(),
        kind: Kind::Codex,
        command: "codex".into(),
        cwd: root.to_string_lossy().into(),
        created_at: 1,
        status: Status::Running,
        status_since: 1,
        window_id: Some("@1".into()),
        pane_id: Some("%1".into()),
        session_id: Some("native-demo".into()),
        last_message: None, waiting_for: None,
        model: None,
        effort: None,
        label: None,
    };
    fs::write(
        home.join("agents/codex-demo.json"),
        serde_json::to_vec(&agent).unwrap(),
    )
    .unwrap();
    // This test binary has one test; other integration binaries have separate environments.
    unsafe {
        std::env::set_var("SHIFU_HOME", &home);
        std::env::set_var("SHIFU_SOCKET", "inbox-fixture");
        std::env::set_var("SHIFU_TMUX", &fake);
        std::env::set_var("INBOX_FIXTURE", &root);
        std::env::set_var("CODEX_HOME", root.join("codex"));
    }
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_shifu"))
            .args(args)
            .output()
            .unwrap()
    };
    let enqueue = |id: &str, text: &str| {
        let out = cli(&["tell", "codex-demo", "--queue", "--id", id, text]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    enqueue("z-first", "first instruction");
    enqueue("a-second", "second instruction");
    fs::write(root.join("screen"), "Working\n› keep my draft\nGPT-6").unwrap();
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert!(!root.join("pastes").exists());
    assert_eq!(
        inbox::receipt(&home, &agent.name, "z-first").unwrap().phase,
        Phase::Queued
    );
    fs::write(
        root.join("screen"),
        "Working\n› Ask Codex to do anything\n\nGPT-6\n? for shortcuts",
    )
    .unwrap();
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("pastes")).unwrap(),
        "first instruction\n"
    );
    assert_eq!(
        inbox::receipt(&home, &agent.name, "z-first").unwrap().phase,
        Phase::Written
    );
    // Native acknowledgement is a separate event; it cannot be inferred from a successful write.
    let mut child = Command::new(env!("CARGO_BIN_EXE_shifu"))
        .args(["hook", "codex", "UserPromptSubmit"])
        .env("SHIFU_ID", &agent.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"native-demo","prompt":"first instruction"}"#)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        inbox::receipt(&home, &agent.name, "z-first").unwrap().phase,
        Phase::Received
    );
    enqueue("z-first", "first instruction"); // same id: no new message
    assert!(
        !cli(&[
            "tell",
            "codex-demo",
            "--queue",
            "--id",
            "z-first",
            "different instruction"
        ])
        .status
        .success()
    );
    inbox::cancel(&home, &agent.name, "a-second").unwrap();
    enqueue("ambiguous", "one attempt");
    fs::write(root.join("fail"), "1").unwrap();
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, &agent.name, "ambiguous")
            .unwrap()
            .phase,
        Phase::Unknown
    );
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("pastes"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    fs::remove_file(root.join("fail")).unwrap();
    enqueue("identity", "do not retarget");
    let mut changed = agent.clone();
    changed.session_id = Some("other-native".into());
    inbox::pump_one(&home, &[changed], &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, &agent.name, "identity")
            .unwrap()
            .phase,
        Phase::Cancelled
    );
    assert!(
        cli(&[
            "tell",
            "codex-demo",
            "--queue",
            "--id",
            "expired",
            "--expires-at",
            "1",
            "too late"
        ])
        .status
        .success()
    );
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, &agent.name, "expired").unwrap().phase,
        Phase::Cancelled
    );
    let mut approval = agent.clone();
    approval.status = Status::NeedsApproval;
    assert!(inbox::enqueue(&home, &approval, "approval", "yes", None, None).is_err());
    assert!(inbox::enqueue(&home, &agent, "control", "\x1b[201~ injected", None, None).is_err());
    let stop = root.join("STOP");
    fs::write(&stop, "").unwrap();
    inbox::enqueue(&home, &agent, "stopped", "do not send", None, Some(&stop)).unwrap();
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, &agent.name, "stopped").unwrap().phase,
        Phase::Cancelled
    );
    fs::remove_file(&stop).unwrap();
    fs::write(
        root.join("screen"),
        "Working\n› Ask Codex to do anything\nGPT-6",
    )
    .unwrap();
    fs::write(root.join("stop-on-paste"), "").unwrap();
    let enters = fs::read_to_string(root.join("enters")).unwrap();
    inbox::enqueue(
        &home,
        &agent,
        "stopped-after-paste",
        "leave this draft unsent",
        None,
        Some(&stop),
    )
    .unwrap();
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, &agent.name, "stopped-after-paste")
            .unwrap()
            .phase,
        Phase::Unknown
    );
    assert_eq!(fs::read_to_string(root.join("enters")).unwrap(), enters);
    fs::remove_file(root.join("stop-on-paste")).unwrap();
    fs::write(
        root.join("screen"),
        "Working\n› Ask Codex to do anything\nGPT-6",
    )
    .unwrap();
    enqueue("native-log", "accepted by native log");
    inbox::pump_one(&home, &[agent.clone()], &Tmux::from_env()).unwrap();
    fs::OpenOptions::new().append(true).open(&native_log).unwrap().write_all(
        b"{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"accepted by native log\"}]}}\n").unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    assert_eq!(
        inbox::receipt(&home, "codex-demo", "native-log")
            .unwrap()
            .phase,
        Phase::Received
    );
    enqueue("repeat-1", "check again");
    enqueue("repeat-2", "check again");
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    let event = serde_json::json!({"type": "response_item", "payload": {
        "type": "message", "role": "user", "content": [{"type": "input_text", "text": "check again"}]
    }});
    let log_input = || writeln!(fs::OpenOptions::new().append(true).open(&native_log).unwrap(), "{event}").unwrap();
    log_input();
    // One native user event confirms the first send. The identical second message is only written so far.
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    assert_eq!(inbox::receipt(&home, &agent.name, "repeat-1").unwrap().phase, Phase::Received);
    assert_eq!(inbox::receipt(&home, &agent.name, "repeat-2").unwrap().phase, Phase::Written);
    log_input();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    assert_eq!(inbox::receipt(&home, &agent.name, "repeat-2").unwrap().phase, Phase::Received);
    // Both identical submissions reach Written before any native evidence exists.
    enqueue("overlap-1", "overlapping same text");
    enqueue("overlap-2", "overlapping same text");
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    inbox::acknowledge(&home, &agent, "overlapping same text").unwrap();
    let overlap = serde_json::json!({"type":"response_item","payload":{
        "type":"message","role":"user","content":[{"type":"input_text","text":"overlapping same text"}]
    }});
    writeln!(fs::OpenOptions::new().append(true).open(&native_log).unwrap(), "{overlap}").unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    for id in ["overlap-1", "overlap-2"] {
        assert_eq!(inbox::receipt(&home, &agent.name, id).unwrap().phase, Phase::Written);
    }
    // A hook and its single transcript row are one receipt, not two.
    inbox::acknowledge(&home, &agent, "overlapping same text").unwrap();
    for id in ["overlap-1", "overlap-2"] {
        assert_eq!(inbox::receipt(&home, &agent.name, id).unwrap().phase, Phase::Written);
    }
    writeln!(fs::OpenOptions::new().append(true).open(&native_log).unwrap(), "{overlap}").unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    for id in ["overlap-1", "overlap-2"] {
        assert_eq!(inbox::receipt(&home, &agent.name, id).unwrap().phase, Phase::Received);
    }
    // Old, fully accounted-for identical sends do not permanently disable hooks.
    enqueue("overlap-3", "overlapping same text");
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    inbox::acknowledge(&home, &agent, "overlapping same text").unwrap();
    assert_eq!(inbox::receipt(&home, &agent.name, "overlap-3").unwrap().phase, Phase::Received);
    // Its delayed transcript cannot confirm the next send, even after the hook.
    enqueue("overlap-4", "overlapping same text");
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(&native_log).unwrap(), "{overlap}").unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    inbox::acknowledge(&home, &agent, "overlapping same text").unwrap();
    assert_eq!(inbox::receipt(&home, &agent.name, "overlap-4").unwrap().phase, Phase::Written);
    writeln!(fs::OpenOptions::new().append(true).open(&native_log).unwrap(), "{overlap}").unwrap();
    inbox::pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
    assert_eq!(inbox::receipt(&home, &agent.name, "overlap-4").unwrap().phase, Phase::Received);
    // A draft holds this conversation's first message but must not starve a different conversation.
    let busy = Agent { name: "codex-busy".into(), pane_id: Some("%2".into()), ..agent.clone() };
    shifu::agent::save(&busy).unwrap();
    inbox::enqueue(&home, &busy, "on-hold", "after your draft", None, None).unwrap();
    enqueue("free", "ready for the other conversation");
    inbox::pump_one(&home, &[busy.clone(), agent.clone()], &Tmux::from_env()).unwrap();
    assert_eq!(inbox::receipt(&home, "codex-busy", "on-hold").unwrap().phase, Phase::Queued);
    assert_eq!(inbox::receipt(&home, "codex-demo", "free").unwrap().phase, Phase::Written);
    // A busy queue gets one turn, then another ready conversation gets its oldest message.
    fs::write(root.join("both-ready"), "").unwrap();
    enqueue("first-turn", "first conversation has more work");
    enqueue("next-turn", "first conversation still has more work");
    let agents = [agent, busy];
    let mut pump = inbox::InboxPump::default();
    let mut tick = || pump.tick(&home, &agents, &Tmux::from_env(), |_, send| send()).unwrap();
    tick();
    assert_eq!(inbox::receipt(&home, "codex-demo", "first-turn").unwrap().phase, Phase::Written);
    tick();
    assert_eq!(inbox::receipt(&home, "codex-busy", "on-hold").unwrap().phase, Phase::Written);
    assert_eq!(inbox::receipt(&home, "codex-demo", "next-turn").unwrap().phase, Phase::Queued);
    tick();
    assert_eq!(inbox::receipt(&home, "codex-demo", "next-turn").unwrap().phase, Phase::Written);
    let submitted = fs::read(root.join("enters")).unwrap();
    fs::write(root.join("edit-after-paste"), "").unwrap();
    inbox::enqueue(&home, &agents[0], "spacing", "Review the blue file.", None, None).unwrap();
    tick();
    assert_eq!(inbox::receipt(&home, "codex-demo", "spacing").unwrap().phase, Phase::Unknown);
    assert_eq!(fs::read(root.join("enters")).unwrap(), submitted);
    fs::remove_dir_all(root).unwrap();
}
