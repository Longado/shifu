# 用本地 Shifu 连接另一台 Mac

[回到 Shifu](../README.md) · [English](REMOTE-RELAY.en.md)

本地 Mac 是控制端，另一台 Mac 运行项目和 Agent。两端主动连接同一个 HTTPS 中继；远端 Mac 不需要开放公网入站端口，也不需要安装 Tailscale。这个入口提供 Agent 终端操作，不提供完整远程桌面或文件传输。

## 下载和首次安装

[下载 remote-v0.3.4-preview.1](https://github.com/Longado/shifu/releases/tag/remote-v0.3.4-preview.1)，选择 `Shifu-remote-v0.3.4-apple-silicon.zip`。这是 M 系列 Mac 的预发布包，包含 `Shifu.app`、匹配的 `bin/shifu`、中英安装说明和文件校验；App 内含远程服务、网页和加密组件。使用预构建包不需要 Rust 或 Node。

还需要自行安装 Homebrew 的 tmux，以及要使用的 Claude Code 或 Codex，并在远端完成 Agent 登录。包内不含 Agent、账号、项目、会话、主机身份或中继接入凭证。

1. 用已有的远程桌面或本机操作完成首次设置。下载 ZIP 和同一 Release 中的 `SHA256SUMS`，在下载目录执行 `shasum -a 256 -c SHA256SUMS`，再解压。
2. 先检查 Mac 使用 Apple Silicon。解压后，在包的目录执行 `shasum -a 256 -c SHA256SUMS`，核对包内文件。
3. 通过 Finder 将 `Shifu.app` 放到个人「应用程序」目录（`~/Applications`）。如果已经安装 Shifu，先退出旧 App，备份旧 App 和 `~/.cargo/bin/shifu` 后再替换；不要覆盖正在运行的 App。
4. 首次安装 CLI 时，在解压目录执行：

   ```sh
   mkdir -p "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   brew install tmux
   ```

   `~/.cargo/bin` 是 App 默认查找 CLI 的位置，使用这个目录不要求安装 Rust。已有 tmux 时跳过安装命令；Homebrew 的安装参考其[官方网站](https://brew.sh/)。
5. 打开 App。此包采用临时签名，尚未公证；若 macOS 拦截，在确认下载来源后按「系统设置 → 隐私与安全性」中的对应提示处理。参见 [Apple 的打开说明](https://support.apple.com/102445)。首次打开、嵌入组件权限和原生输入仍需在目标 Mac 上确认。
6. 安装并登录所需 Agent，在远端 Shifu 中打开一个项目。首次通过 Shifu 启动 Codex 时，检查它展示的 hooks 后决定是否信任。

Shifu 默认以免确认模式启动 Claude/Codex。获批设备可以向远端 Agent 输入并创建会话；Agent 能执行的操作取决于它的登录和运行权限。首次连接先使用专门的测试项目。

## 中继与配对

先准备两边都能访问的 HTTPS 中继和对应的接入凭证。它们由自己的服务器管理员提供；仓库和安装包不提供公共中继账号。可自行部署：

```sh
npm --prefix app ci
npm --prefix app run build:remote
export SHIFU_RELAY_DOMAIN=relay.example.com
docker compose -f remote/compose.yaml up -d --build
```

此命令用于独立服务器：先配置自己的域名及 DNS，确保 80/443 可用；已有 HTTPS 服务时按[已有服务器部署说明](../remote/deploy/README.md)接入，不直接占用现有端口。Docker 中继的凭证首次生成于持久卷中的 `/data/join-key`。通过既有安全管理路径将凭证交给远端 Mac，不要写入网址、仓库或公开截图。中继网页也应来自自己信任的服务器。

1. **在远端 Mac：** Shifu 底栏「远程连接 → 允许其他设备访问本机…」，填写自己的中继 HTTPS 地址和接入凭证，启用后等待「中继已连接」。可另选「登录后启动」。
2. **在远端 Mac：** 点「配对设备」，临近连接时生成配对地址。地址十分钟有效，只交给自己的控制设备。
3. **在本地 Mac：** Shifu 底栏「远程连接」，填写名称和配对地址，点「连接远程 Mac」。本地作为控制端时不需要启用本机被控服务。
4. **回到远端 Mac：** 检查申请并批准「Shifu 控制端」。本地随后可以选择远端已有会话。

每台 Mac 生成自己的身份；不要复制另一台机器的 `~/.shifu`。本地「忘记」清除保存的连接资料；远端「撤销」使对应设备失去权限。保留原来的远程桌面作为维护入口。

## 日常使用

双方使用本版本后，本地连接远端，可点「新建会话」，选择远端已登记项目及已安装的 Claude/Codex，再点「创建并进入」。没有项目时先在远端 Shifu 打开项目；没有 Agent 时先安装并登录。模型及思考档位沿用远端 CLI 默认值。

「断开并返回本机」只断开当前访问；远端 Mac 开机联网、tmux 与 Agent 正常运行时，任务会继续。网络重连不会自动重发输入或重新创建会话；发送或创建结果未确认时，先回看终端或刷新会话列表。重启、断电和系统睡眠与网络断线不同，不能保证原进程继续。

远端需保持运行和联网。杯子菜单可设置保持运行；合盖运行和系统重启后的解锁、登录及服务恢复需要在该机上另行确认。

## 预发布范围

- 仅提供 Apple Silicon 包；未提供 Intel 包或 Apple 公证。
- 远程逻辑已在隔离环境配合测试 Agent 演练；发布包的原生首次安装、真实 Claude/Codex TUI、跨国网络与中文输入仍待目标机器确认。
- iPhone/Safari 和蜂窝切网尚未实测。旧的[本机手机入口](REMOTE.md)继续独立提供，不代表本次手机接入已完成。
- 下载和安装不会自动配置中继、登录 Agent 或替你批准配对。
- 此包面向远程 Agent 编程；独立电脑操作辅助程序需按[电脑操作说明](COMPUTER-USE.md)另行准备。
