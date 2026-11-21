# Installation and use

[Back to Shifu](../README.en.md)　[中文](INSTALL.md)

Shifu provides an [Apple Silicon preview package](https://github.com/Nalu-Lab/shifu/releases/tag/remote-v0.4.0-preview.1) containing the app, matching CLI and setup instructions. Rust is not needed for ordinary sessions; StepFun additionally needs Node/Pi. Install tmux and your agent separately and sign into the agent. Follow [remote setup and pairing](REMOTE-RELAY.en.md) for first installation.

Use the download for ordinary sessions; source-build instructions are in the developer section below.

Claude, Codex, CodeBuddy and Kimi start in no-confirmation mode by default, using flags such as `--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox` and `--yolo`. They can edit files and run commands without individual approval. Review `src/launch.rs` before building if you want different defaults.

The app is signed with Shifu's own fixed certificate (not one issued by Apple) and is not notarized. If macOS blocks it the first time, confirm its source and follow the app-specific prompt in System Settings → Privacy & Security.

## Quick start

[Download the Apple Silicon preview](https://github.com/Nalu-Lab/shifu/releases/tag/remote-v0.4.0-preview.1), including the app, matching CLI and setup instructions. Ordinary sessions need no Rust; install tmux and sign into your agent separately. StepFun requires [Node/Pi and subscription-key setup](STEPFUN.md#english).

### Option A: let your AI install it

Paste this into Claude Code or Codex:

```text
Install Shifu (https://github.com/Nalu-Lab/shifu) for me. Read README.en.md and docs/INSTALL.en.md first.
1. Confirm this is an Apple Silicon Mac. Check that tmux and my chosen agent are installed and signed in; tell me what is missing.
2. Download the ZIP and SHA256SUMS from the same release linked in the README. Verify the download and extracted-file checksums. Use the packaged app and CLI by default; do not install Rust or build from source.
3. Follow the package instructions: put Shifu.app in ~/Applications and its matching bin/shifu in ~/.cargo/bin/shifu. Add ~/.cargo/bin to the current shell PATH, verify shifu --help, and explain the full-path option for new terminals. For an existing installation, explain the brief quit needed for the upgrade and preserve the old app, CLI and sessions. Do not overwrite a running app.
4. Explain that the app is signed with Shifu's own certificate, not notarized by Apple, and starts agents in no-confirmation mode. Leave system security prompts to me. Configure StepFun, computer use, a remote relay or background follow-up only if I request them.
5. Help me choose a project and start my first agent conversation. On the first Codex launch, let me review its hooks before deciding whether to trust them.
```

### Option B: install it yourself

1. Download the ZIP and `SHA256SUMS` from the same release. Run `shasum -a 256 -c SHA256SUMS` in the download directory before extracting.
2. For an existing installation, quit it and back up the old app and CLI first; use the app and CLI from the same package (the app keeps a notice at the bottom of the sidebar while they do not match). In the extracted package directory, verify its contents before installing the CLI:

   ```bash
   shasum -a 256 -c SHA256SUMS
   mkdir -p "$HOME/Applications" "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   rm -rf "$HOME/.cargo/bin/web" && ditto bin/web "$HOME/.cargo/bin/web"   # runtime for shifu web
   export PATH="$HOME/.cargo/bin:$PATH"
   brew install tmux   # Skip if tmux is already installed
   ```

3. After successful verification, move `Shifu.app` to `~/Applications`. Open it, choose a project and a signed-in agent, and start your first conversation. See the [installation guide](INSTALL.en.md) for details; use [remote setup and pairing](REMOTE-RELAY.en.md) when connecting another Mac.

- The app is signed with Shifu's own fixed certificate and not notarized. If macOS blocks it, confirm the source and follow the app-specific prompt in System Settings → Privacy & Security.
- Codex asks you to review its hooks on first launch; read them before deciding whether to trust them. Keeping the CLI installation path avoids repeating setup.
- The PATH command applies to the current terminal. In a new terminal without that PATH setting, use `"$HOME/.cargo/bin/shifu"` or add that directory to your shell configuration.
- CLI only: `shifu` opens the workbench, `shifu new claude ~/proj` starts an agent, `shifu ls` lists sessions, and `shifu jump` opens a waiting conversation.

<details>
<summary>Build from source (developers)</summary>

Requires Rust and Node 20.19 or 22.12+.

```bash
brew install tmux
git clone https://github.com/Nalu-Lab/shifu && cd shifu
cargo install --path .
cd app && npm install
npm run build:desktop -- --debug
# Open the app path printed by the build log in the managed .noindex cache
```

</details>

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

To check a macOS bundle against its companion CLI before opening it, run `"/path/to/Shifu.app/Contents/MacOS/shifu-app" --source-version` and `/path/to/shifu version`. The two commands print the same fingerprint when their shared Rust source matches.

```bash
cargo test && cargo clippy -- -D warnings           # CLI and hooks
cargo build --release && bash scripts/smoke.sh       # smoke test: own socket, temp folder, fake agents
cd app && npm test && npm run build                  # frontend tests and type check
cd app/src-tauri && cargo test                       # app backend
cd app && npm run tauri dev                          # dev mode
```

Set `SHIFU_APP_HOME` and `SHIFU_APP_SOCKET` while developing so test data stays apart from the agents you are using.


StepFun additionally requires Node/Pi and a subscription key: [setup](STEPFUN.md#english). Update the app and matching CLI together. The [autopilot script](AUTOPILOT.md#english) is not started by installation.

Current source also provides [Qwen Code / Token Plan](QWEN.md#english), [scheduled daily-analysis catch-up](DAILY-ANALYSIS.md#english), and [worktree handoffs / session recovery](WORKSPACES.md). These source additions are not in the release package linked above; build the app and CLI from the same revision when using them.
