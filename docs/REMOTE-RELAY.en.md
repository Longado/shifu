# Connect to another Mac from local Shifu

[Back to Shifu](../README.en.md) · [中文](REMOTE-RELAY.md)

Your local Mac is the controller; the other Mac runs projects and agents. Both connect outward to the same HTTPS relay. The remote Mac needs no public inbound port or Tailscale installation. This provides agent terminal control, not a full remote desktop or file transfer.

## Download and first installation

Open [remote-v0.3.4-preview.1](https://github.com/Longado/shifu/releases/tag/remote-v0.3.4-preview.1) and download `Shifu-remote-v0.3.4-apple-silicon.zip`. This preview is for Apple Silicon Macs. It contains `Shifu.app`, the matching `bin/shifu`, bilingual setup instructions and file checksums. The app includes the remote gateway, web client and encryption component. Rust and Node are not needed to use this prebuilt package.

Install Homebrew tmux and your chosen Claude Code or Codex separately, and sign into the agent on the remote Mac. No agents, accounts, projects, sessions, machine identities or relay credentials are included.

1. Use an existing remote desktop connection or local access for initial setup. Download the ZIP and `SHA256SUMS` from the same release. Run `shasum -a 256 -c SHA256SUMS` in the download folder, then unzip.
2. Check that the Mac uses Apple Silicon. Run `shasum -a 256 -c SHA256SUMS` inside the extracted package to verify its files.
3. In Finder, move `Shifu.app` into your personal Applications folder (`~/Applications`). If Shifu is already installed, quit the old app and back up both it and `~/.cargo/bin/shifu` before replacing them. Do not replace a running app.
4. For a first CLI installation, run these commands inside the extracted package:

   ```sh
   mkdir -p "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   brew install tmux
   ```

   The app looks for the CLI in `~/.cargo/bin` by default; this does not require Rust. Skip the tmux command if it is already installed. See the [Homebrew website](https://brew.sh/) for Homebrew installation.
5. Open the app. This package is ad-hoc signed and not notarized. If macOS blocks it, check the download source and follow the app-specific prompt under System Settings → Privacy & Security. See [Apple's instructions](https://support.apple.com/102445). Initial launch, embedded component permissions and native input still need confirmation on the target Mac.
6. Install and sign into the chosen agent, then open a project in remote Shifu. When first starting Codex through Shifu, review its displayed hooks before deciding whether to trust them.

Shifu starts Claude/Codex in no-confirmation mode by default. Approved devices can enter input into remote agents and create sessions; what an agent can do depends on its login and runtime permissions. Use a dedicated test project for the first connection.

## Relay and pairing

Prepare an HTTPS relay reachable from both ends and its join credential. Your own server administrator supplies these; the repository and package do not provide a public relay account. For self-hosting:

```sh
npm --prefix app ci
npm --prefix app run build:remote
export SHIFU_RELAY_DOMAIN=relay.example.com
docker compose -f remote/compose.yaml up -d --build
```

These commands assume a dedicated server, your own domain and DNS, and available ports 80/443. With an existing HTTPS service, use the [shared-server instructions](../remote/deploy/README.md) instead of taking its ports. The Docker relay creates its credential at `/data/join-key` in its persistent volume. Transfer it to the remote Mac through your existing secure administration route; never put it in a URL, repository or public screenshot. Use a relay whose web client you trust.

1. **Remote Mac:** in Shifu's bottom bar, select “远程连接 → 允许其他设备访问本机…”. Enter your relay HTTPS URL and join credential, enable it and wait for “中继已连接”. Login startup is optional.
2. **Remote Mac:** select “配对设备” shortly before connecting. The pairing URL expires after ten minutes; share it only with your own controller.
3. **Local Mac:** select “远程连接”, enter a name and pairing URL, then “连接远程 Mac”. A controller does not need to enable inbound access to itself.
4. **Remote Mac:** review and approve “Shifu 控制端”. The local Mac can then select a remote session.

Each Mac generates its own identity; do not copy another machine's `~/.shifu` directory. “忘记” removes locally saved connection details; “撤销” on the remote host revokes that device. Keep your existing remote desktop connection for maintenance.

## Daily use

With this version on both Macs, connect locally and choose “新建会话”, select a registered remote project and installed Claude/Codex, then “创建并进入”. Register a project in remote Shifu first; install and sign into any missing agent. Model and thinking options use that remote CLI's defaults.

“断开并返回本机” closes the current connection. Tasks keep running while the remote Mac, tmux and agent remain running. Reconnection does not automatically resend input or repeat session creation. If an input or creation result is uncertain, inspect the terminal or refresh the session list first. A reboot, power loss or system sleep differs from a network disconnection and may end or suspend the process.

Keep the remote Mac running and online. The cup menu can keep it awake; closed-lid operation and unlocking, login and service recovery after reboot need separate confirmation on that machine.

## Preview limits

- Apple Silicon only; no Intel package or Apple notarization.
- Remote logic has been exercised in isolation with test agents. Native first installation of the release package, real Claude/Codex TUIs, cross-country networking and native Chinese input still need confirmation on the target machines.
- iPhone/Safari and cellular network switching have not been tested. The older [local phone gateway](REMOTE.md) is still separate and does not establish completion of this phone workflow.
- Downloading and installing does not configure your relay, sign into agents or approve pairing automatically.
- This package targets remote agent coding. Prepare the independent computer-use helper separately using the [computer-use guide](COMPUTER-USE.md).
