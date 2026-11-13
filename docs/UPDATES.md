# 更新记录

[回到 Shifu](../README.md)　[English](#english)

## 2026-11-13

- 应用图标更新为「方扣」：两个相扣的几何构件，深色底配白色图形。

## 2026-11-12

- Markdown 文件可以直接编辑、保存，支持 ⌘S、草稿恢复及文件冲突提示；拖标签到左右约三分之一处即可分屏。
- 标签点叉每次都可选「收起，继续运行」或「结束」；跨任务查看的会话仍留在原任务。
- 杯子菜单提供正常睡眠、保持运行、屏幕常亮、定时和合盖运行；包含低电量、过热暂停及异常退出恢复。[使用说明](STEAMCUP.md)。
- 手机网页可查看并回复电脑上已有的终端，支持中文输入和断线重连。可选本机服务需要单独安装，外网连接需配置私有网络。[安装说明](REMOTE.md)。
- 电脑操作新增 `shifu computer guide`，改进多窗口、系统选文件框和焦点检查；操作后可只返回变化的控件，动作结果不确定时保留回执。默认后台仍可能弹出应用自己的窗口。
- 新建任务支持 Antigravity；可选本机账号的模型和思考档位，并恢复原对话。点餐按钮可连接已有的本机点餐服务；本仓库不包含点餐服务及账号。

## 2026-11-11

- 电脑操作支持独立小输入窗口；按窗口 ID 或索引切换时会激活指定目标，临时前台读取后恢复原焦点窗口。附带可手动运行的 Finder/Safari 真实任务检查。

- Antigravity 额度监测：通过当前已登录的 `agy`（1.1.11+）读取[官方 `/usage` 报告](https://antigravity.google/docs/cli/commands/usage/)，底栏分 Gemini 与 Claude/GPT 显示已用周额度、5小时额度和重置时间，每5分钟刷新；失败会标明旧数据或不可用。无需 Shifu 保存账号凭据。

- 独立的 macOS 电脑操作：`shifu computer` 可读取窗口、截图、点击、输入、滚动和拖动，运行不依赖 Orca。需要给 Shifu 自有辅助程序授予系统权限；[安装与参数说明](COMPUTER-USE.md)。
- 控件可以按文字筛选、限制返回数量，等待目标文字出现；Safari 可显式选择短暂前台读取后切回。文件条件未满足会返回失败，控件赋值成功不等于文件任务完成。
- 项目经理可以选择 agent，查看当前对话状态并通过本地协作命令传话；搜索可查当前对话及其内容，侧栏可隐藏，支持上下分屏。
- 底栏显示额度与进程占用，杯子按钮控制保持运行或保持屏幕开启；退出后释放保持唤醒。文件标签支持 HTML、音视频预览。

Finder 的独立改名输入框现已可读取、输入并提交；操作结果仍应核对文件路径。浏览器后台可能只有工具栏，等待文字时应选正文中的明确片段；目前没有 DOM 操作通道。

## English

### November 13, 2026

- Updated the app icon to the corner-joint mark: two interlocking geometric pieces in white on a dark background.

### November 12, 2026

- Edit and save Markdown in file tabs, with Cmd+S, draft recovery and conflict detection. Drag a tab into the outer third of the content area to split it.
- Closing a tab always offers to hide it while the agent keeps running, or end it. Agents viewed across tasks keep their original task membership.
- The cup menu adds timers and closed-lid operation, with low-battery and thermal pauses and recovery after an unexpected app exit. See the [power guide](STEAMCUP.md).
- An optional local service lets a phone view and reply to existing desktop terminals, with Chinese input and reconnection. Remote access requires a private network you configure. See [setup](REMOTE.md).
- `shifu computer guide` documents the shared computer-use entry point. Window selection, native file pickers and focus checks are improved; action readbacks can return changed controls while preserving receipts when the outcome is uncertain. Background operation can still trigger an application's own dialogs.
- Create and restore Antigravity conversations with locally available models and thinking levels. A meal-ordering button can connect to an existing local service; that service and its accounts are not included.

### November 11, 2026

- Computer use now recognizes standalone input windows, activates the selected window by ID or index, and restores the previous focused window after a temporary foreground read. An opt-in Finder/Safari task check is included.

- Antigravity quota monitoring uses the signed-in `agy` CLI (1.1.11+) and its [official `/usage` report](https://antigravity.google/docs/cli/commands/usage/). The footer shows used weekly and five-hour quotas for Gemini and Claude/GPT separately, with reset times and a five-minute refresh interval. Failed refreshes are marked stale or unavailable; Shifu does not store account credentials.

- Independent macOS computer use through `shifu computer`: inspect windows, capture screenshots, click, type, scroll and drag without running Orca. Grant permissions to Shifu's own helper; see the [setup and command guide](COMPUTER-USE.md).
- Filter controls by text, limit output, wait for visible text, and check file path conditions. An explicit foreground fallback briefly reads the window and restores the previous app. A changed control value does not prove that a file operation completed.
- Choose an agent for the project manager, inspect conversation status and relay messages through local commands. Search active conversations, hide the sidebar and split panes vertically.
- A bottom bar shows usage and process memory, with controls to keep the system or display awake. File tabs support HTML and audio/video previews.

Finder standalone rename editors can now be observed, edited and committed; verify the resulting file paths. Background browser reads may return only toolbars; wait for specific body text. There is no browser DOM channel yet.
