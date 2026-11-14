# Installation and use

[Back to Shifu](../README.en.md)　[中文](INSTALL.md)

Shifu provides an [Apple Silicon preview package](https://github.com/Longado/shifu/releases/tag/remote-v0.3.4-preview.1) containing the app, matching CLI and setup instructions. Rust/Node are not needed to use it; install tmux and your agent separately and sign into the agent. Follow [remote setup and pairing](REMOTE-RELAY.en.md) for first installation.

The source-build instructions below require Homebrew tmux, Rust, and Node 20.19 or 22.12+.

Claude, Codex, CodeBuddy and Kimi start in no-confirmation mode by default, using flags such as `--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox` and `--yolo`. They can edit files and run commands without individual approval. Review `src/launch.rs` before building if you want different defaults.

The app is ad-hoc signed and is not notarized. If macOS blocks it the first time, confirm its source and follow the app-specific prompt in System Settings → Privacy & Security.

## Quick start

### Option A: let your AI install it

Paste this into Claude Code or Codex:

```text
Install Shifu (https://github.com/Longado/shifu) for me. Read its README.en.md and docs/ARCHITECTURE.md first.
1. Check that this Mac has Homebrew tmux, Rust (cargo) and Node 20.19 or 22.12+; tell me how to install what is missing.
2. Clone the repo and run `cargo install --path .` in its root to install the shifu CLI (the app's hooks call ~/.cargo/bin/shifu).
3. In app/, run `npm install`, then `npm run tauri build -- --debug`.
4. Tell me where Shifu.app ended up, and remind me that it is not notarized (follow the app-specific prompt in Privacy & Security if blocked) and that it starts agents in no-confirmation mode.
5. Remind me that the first time I open Codex in Shifu, Codex asks whether to trust the hooks: choose "Trust all".
```

### Option B: install it yourself

```bash
brew install tmux
git clone https://github.com/Longado/shifu && cd shifu
cargo install --path .                      # the shifu CLI, into ~/.cargo/bin
cd app && npm install
npm run tauri build -- --debug              # builds src-tauri/target/debug/bundle/macos/Shifu.app
open src-tauri/target/debug/bundle/macos/Shifu.app
```

- The app is ad-hoc signed and not notarized. If macOS blocks it the first time, confirm its source and follow the app-specific prompt in System Settings → Privacy & Security.
- The first time you start Codex in Shifu it shows "Hooks need review"; choose **Trust all and continue**. It won't ask again as long as the `shifu` install path stays the same.
- CLI only: `shifu` opens the workbench, `shifu new claude ~/proj` starts an agent, `shifu ls` lists them, `shifu jump` goes to the one that has waited longest.

## Shortcuts

| Key | Does |
|---|---|
| Cmd+N | New task |
| Cmd+T | Add an agent to the current task |
| Cmd+W | Close the current panel (the agent keeps running) |
| Cmd+J | Jump to the agent that has waited longest |
| Cmd+, | Appearance |
| Double-click a tab | Rename the agent; leave it empty to go back to the topic name |
| × on a tab | Choose to hide the tab while the agent keeps running, or end it; ending a running agent needs another confirmation |

## Privacy

- Shifu provides no public hosted account and collects no telemetry. State, tasks and layouts stay on your Mac in `~/.shifu/app` (`~/.shifu` for the CLI). The local phone gateway listens on `127.0.0.1`; the new remote module can use your own HTTPS relay, with terminal data and requests in an end-to-end encrypted channel. Use a relay whose web client you trust. [Connection guide](REMOTE-RELAY.en.md).
- Hooks for Claude Code, Codex and CodeBuddy are passed as launch arguments to the processes Shifu starts. Shifu does **not** change your `~/.claude/settings.json` or `~/.codex/config.toml`.
- **Kimi is the exception**: Kimi only reads its own config file, so Shifu writes hooks into `~/.kimi-code/config.toml`. Your own hooks stay, and the file is backed up to `config.toml.before-shifu` before the first write. Kimi sessions started outside Shifu call these hooks too, but they do nothing when the session was not started by Shifu.
- Quotas: Claude's come from the data Claude hands to its statusLine (your own statusLine still shows as before); Codex's and CodeBuddy's are read from local session logs. Kimi's are queried using Kimi Code's current local login token without storing a separate copy. Antigravity usage comes from the signed-in `agy` CLI.
- Shifu uses its own tmux socket (`shifu-app`, `shifu` for the CLI) and leaves your usual tmux alone.

## Known issues

- Only used on macOS (Apple Silicon). Linux and Windows are untried; notifications and drag and drop use macOS interfaces.
- If a `git` command hangs in a project folder (for example while macOS waits for a "Desktop folder" permission), the whole window freezes until git returns. The cause is known: the branch name is read with a lock held. Not fixed yet.
- A long path wrapped over three or more lines in a terminal can't be opened by clicking its later lines. Cause not found yet.
- Expired Kimi Code credentials prevent quota refreshes. Shifu keeps the last result until running Kimi refreshes its login.
- Phone access from outside the Mac requires a separately configured private network.
- Two optional local data sources are wired in: the progress page reads each project's `GOALS.md` and WorkBuddy's local tasks, and new-reply notifications first check a local judgement log from a tool called Jev. Without those files they are skipped.

## Build with us

**When many agents work at once, how does a person get called only when they are really needed?**

| If you like… | You could |
|---|---|
| Rust and concurrency | Move the branch-name git call out of the lock and give it a timeout, fixing the window freeze above |
| Terminals and xterm.js | Fix clicking long wrapped paths; add search inside a terminal |
| Wiring up agents | Gemini CLI, Cursor CLI and others: adapt their hooks so their state shows in the sidebar |
| Cross-platform work | Notifications, drag and drop and packaging on Linux |
| Interaction design | How to order "waiting for you", when to notify and when not to interrupt |

Issues and pull requests are welcome.

## Development

```bash
cargo test && cargo clippy -- -D warnings           # CLI and hooks
cargo build --release && bash scripts/smoke.sh       # smoke test: own socket, temp folder, fake agents
cd app && npm test && npm run build                  # frontend tests and type check
cd app/src-tauri && cargo test                       # app backend
cd app && npm run tauri dev                          # dev mode
```

Set `SHIFU_APP_HOME` and `SHIFU_APP_SOCKET` while developing so test data stays apart from the agents you are using.
