# Shifu {{VERSION}} preview · Apple Silicon

The app, CLI and optional Computer Use helper are a matching set. Quit Shifu before replacing it and back up the old app and CLI. Keep existing session and settings directories.

1. Run `shasum -a 256 -c SHA256SUMS` next to the downloaded archive; unzip and run it again inside this directory.
2. Use Finder to copy `Shifu.app` into `~/Applications`; create that folder for a first installation.
3. Install the matching CLI from this directory:

   ```sh
   mkdir -p "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   ```

4. Install tmux and your chosen agent separately and sign into the agent. Ordinary sessions need no Rust toolchain. Open the app and follow [remote setup and pairing](docs/REMOTE-RELAY.en.md). The package is ad-hoc signed and not notarized. If blocked by macOS, verify its source and follow the app-specific Privacy & Security prompt.
5. StepFun uses Step Plan subscriptions and additionally requires Node.js 20.6+, isolated Pi 0.73.1 and key setup. See [STEPFUN.md](docs/STEPFUN.md), including desktop-vault compatibility and standalone CLI environment injection. Neither runtime nor keys are bundled.

Optional Computer Use: back up an existing `~/Applications/Shifu Computer Use.app`; stop its bridge with the installed CLI's `shifu computer stop` before replacement. In Finder, copy `helpers/Shifu Computer Use.app` into `~/Applications`. Do not launch a development copy from the package. Run `shifu computer doctor`, then `shifu computer permissions` only if needed; set Accessibility and Screen Recording permissions yourself. See the [guide](docs/COMPUTER-USE.md). Skip this helper if unused.

The independent `scripts/autopilot.py` needs Python 3.9+, signed-in Codex and a reviewed run configuration. It follows existing Codex sessions, not StepFun. Installation does not start it: read [AUTOPILOT.md](docs/AUTOPILOT.md). Browser Use requires a separate runtime installed from source as described in [BROWSER-USE.md](docs/BROWSER-USE.md).

No accounts, projects, sessions, device identities, server credentials, Pi/Node or browser runtime are included. Native first installation, physical remote Macs and cross-country input still require target-device confirmation. This package does not automatically upgrade a relay or another device.
