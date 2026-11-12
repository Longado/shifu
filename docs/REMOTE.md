# 手机连接 Shifu

手机连接电脑上已经运行的 Shifu 对话。电脑运行 Agent，手机显示同一个终端并发送输入；手机关闭页面或短暂断网后，Agent 继续运行。

## 本机启动

在项目目录安装一次：

```sh
bash scripts/install-remote.sh
shifu remote start
```

电脑浏览器打开 `http://127.0.0.1:3842`。在电脑终端执行下列命令查看访问码，再填入网页：

```sh
shifu remote code
```

访问码只在本机显示，不放进网址。网页登录有效 12 小时，重启远程服务后需重新登录。服务只监听本机地址。

## 手机从外网连接

1. 在电脑和手机安装 [Tailscale](https://tailscale.com/download)，登录同一个账号并连接。macOS 的网络扩展授权及账号登录由本人完成。
2. 在电脑终端运行：

   ```sh
   tailscale serve status
   tailscale serve --bg 3842
   ```

   先看已有配置；若已用 Serve 分享其他服务，保留原入口并为 Shifu 选择另一个 HTTPS 端口，例如 `tailscale serve --bg --https=8443 3842`。
3. 将命令显示的 `https://…ts.net` 地址在手机浏览器打开，填写电脑上的 Shifu 访问码。可用浏览器“添加到主屏幕”保留入口。

这里使用 Tailscale Serve 的私网 HTTPS 入口，电脑和手机都需要连接 Tailscale。参考：[Serve 官方说明](https://tailscale.com/docs/features/tailscale-serve)、[命令说明](https://tailscale.com/docs/reference/tailscale-cli/serve)。

## 在手机上操作

- 对话按任务分组，显示运行、等你回复、等你批准等状态；点开后显示原终端。
- 在底部回复框输入中文或多行文字，点“发送”。收到终端写入回执后清空原稿，等待期间继续编辑的新文字会保留。
- 批准请求按原 Agent 的界面操作，使用终端键盘或下面的方向键、Enter、Esc 等按键。
- 原终端保持桌面的宽度，手机可以横向滑动查看。远程客户端不会把另一个已连接的桌面客户端挤窄。
- 页面自动尝试重连。断线期间不能发送，未确认的回复保留在当前页面；查看终端后再决定是否重发。页面不会自动重发输入。
- “退出”撤销这次登录并关闭连接，原 Agent 继续运行。

## 停止与更新

```sh
shifu remote status
shifu remote stop
```

停止服务会关闭所有远程连接，保留 Shifu 原会话。网页及独立服务安装在 `~/.shifu/remote`，日志为 `~/.shifu/remote/server.log`。访问码保存在仅当前用户可读的文件中，不要分享访问码或整个目录。

更新前先停止远程入口，再重新运行安装命令。安装保留旧的 CLI、远程服务和网页组件，不替换 Shifu.app。

电脑需要开机、联网并保持运行；休眠时暂时无法连接，可用 Shifu 现有杯子按钮选择保持运行。重启电脑后需重新运行 `shifu remote start`。首版不包括推送、文件传输、远程桌面和自有扫码配对/中继。

## 开发入口

```sh
cargo build --bin shifu
cargo build --manifest-path remote/Cargo.toml
npm --prefix app run build:remote
node remote/tests/acceptance.cjs
```

接受检查使用独立 tmux socket、自己的测试程序、状态目录和浏览器上下文，不向真实 Agent 输入文字。截图和结果默认在 `target/remote-acceptance/`。
