# Shifu web：多个 Agent 并行查网页

`shifu web` 让几个 Agent 同时在各自独立的无界面网页里查资料。同时开几个由本机空闲内存决定，满了自动排队，Agent 查完自己释放。它不接管你自己的浏览器，也不保留登录；需要可见窗口、登录状态或截图时用 [`shifu browser`](BROWSER-USE.md)。

## 安装与启动

需要 Node.js 18+，以及一个本机已在运行的 Playwright 无界面 Chromium 服务端（例如 `chromium.launchServer({ headless: true, port: 9339, wsPath: 'pw' })`）。`shifu web` 不自行下载或启动浏览器。

```bash
cargo install --path . --locked --force
npm --prefix web ci --ignore-scripts      # 只装一个固定版本的 playwright-core
shifu web serve --endpoint ws://127.0.0.1:9339/pw
```

安装包里 `shifu` 旁边的 `web/cli.mjs` 会被优先使用；也可用 `SHIFU_WEB_RUNTIME` 指定。运行目录默认 `~/.shifu/web`，可用 `SHIFU_WEB_HOME` 切换。客户端与服务端的 Playwright 版本须一致。

## 能同时开几个

取当前可用内存的四分之一，按每个任务 256 MiB 折算，再不超过 CPU 核数，至少 1 个。例如 32 GB、10 核、约七成内存空闲时为 10 个。`shifu web status` 的 `capacity` 显示上限、依据、已用和排队数；`SHIFU_WEB_MAX_TASKS`（1 到 32）可固定上限。

满额时 `open` 最多排队 30 秒，有任务释放或内存回升就继续，仍满返回 `CAPACITY_REACHED`；加 `"wait":false` 不排队。

## 一个 Agent 的查资料流程

每条请求是一行 JSON，经 stdin 传入：`printf '%s\n' '<JSON>' | shifu web request`。

1. `{"method":"open","task":"t1","url":"https://..."}`：建本任务自己的独立页面。
2. `{"method":"join","task":"t1","label":"A"}` 得到 `actorId`；`{"method":"claim","task":"t1","actorId":"..."}` 得到 `lease`。
3. 读：`{"method":"find",...,"queries":["关键词"],"scope":{"role":"main","name":""}}` 只返回关键词附近的正文片段与来源网址；`observe` 返回页面结构（ARIA）。
4. 换页：`{"method":"act",...,"requestId":"nav-1","sequence":1,"operation":{"type":"navigate","url":"..."}}`。
5. 查完：`{"method":"close","task":"t1","actorId":"...","lease":{...},"discard":true}` 自行释放，排队的任务随即继续。

页面响应开始就算打开成功，最多再等 5 秒加载；`loaded:false` 表示仍在加载，已到正文可读。页面头部资源迟迟不到、正文区域建不起来时，`find` 改读已收到的 HTML 正文（返回 `source:"html"`），不执行脚本；`observe` 返回 `PAGE_NOT_READY`。跳转失败返回 `navigation_failed`，读取超时返回 `READ_FAILED`，都不影响任务继续使用。

## 填写、点击与本人控制

`fill_form`（textbox）和 `click` 需要当前观察里唯一出现的 role/name。它们结果不明时任务暂停，旧动作不自动重发，只能由本人用 `shifu web request --control` 核对后 `resume` 或 `close`。同一任务可由多个执行者排队交接（`wait`、`yield`），旧执行者交出后不能再写。

本工具只记录任务和页面编号及执行阶段，不保存页面正文、填写内容、Cookie 或截图。它不是多用户安全沙箱：同一系统用户的程序都能访问本机控制入口。
