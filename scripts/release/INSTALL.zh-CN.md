# Shifu {{VERSION}} 预发布包 · Apple Silicon

这是 App、CLI 和可选电脑操作辅助程序的配套版本。不要覆盖正在运行的 App；安装前退出 Shifu 并保留旧 App/CLI 备份，现有会话和设置目录无需删除。

1. 在下载目录运行 `shasum -a 256 -c SHA256SUMS`，解压后在本目录再次运行同一命令。
2. 通过 Finder 将 `Shifu.app` 放到 `~/Applications`，首次安装需创建该目录。
3. 在本目录安装匹配 CLI：

   ```sh
   mkdir -p "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   rm -rf "$HOME/.cargo/bin/web" && ditto bin/web "$HOME/.cargo/bin/web"
   ```

4. 普通会话另需 tmux、对应 Agent 及登录；预构建 App 不需要 Rust。打开 App 后按 [远程安装与配对](docs/REMOTE-RELAY.md) 设置。此包用 Shifu 自己的固定证书签名、未经 Apple 公证；从 0.4.1 起各版证书相同，已给的辅助功能和屏幕录制授权在升级后保留；macOS 拦截时先核实下载来源，再按系统“隐私与安全性”的对应提示处理。
5. StepFun 使用 Step Plan 订阅入口，另需 Node.js 20.6+、独立 Pi 0.73.1 和密钥设置，见 [STEPFUN.md](docs/STEPFUN.md)。这些运行时与密钥不在包内。桌面密钥库兼容要求也在该文档中；CLI 支持环境注入。

可选电脑操作：先备份已有 `~/Applications/Shifu Computer Use.app`；替换前用已安装 CLI 执行 `shifu computer stop` 停止转接程序，再通过 Finder 将 `helpers/Shifu Computer Use.app` 放到 `~/Applications`。不要从包内直接启动开发副本。运行 `shifu computer doctor` 查看状态，需要时再运行 `shifu computer permissions`，由本人设置辅助功能与屏幕录制权限。见 [使用指南](docs/COMPUTER-USE.md)。不使用电脑操作可跳过。

`scripts/autopilot.py` 是独立后台托管入口，需 Python 3.9+、已登录 Codex 和审阅后的本轮配置；只支持现有 Codex 会话，不支持 StepFun 托管。安装不会启动托管，见 [AUTOPILOT.md](docs/AUTOPILOT.md)。Browser Use 的独立运行时需从源码按 [BROWSER-USE.md](docs/BROWSER-USE.md) 安装。 `shifu web` 网页任务池的运行文件在 `bin/web`，随 CLI 一起安装；另需 Node.js 18+ 和本机已运行的 Playwright 无界面 Chromium 服务端，见 [WEB-POOL.md](docs/WEB-POOL.md)。

本包不含账号、项目、会话、设备身份、服务器凭证、Pi/Node 或浏览器运行时。原生首次安装、真实远端 Mac 和跨国输入仍需在目标设备确认；本版本不自动升级云中继或其他设备。
