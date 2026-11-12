# Shifu 内的 Steamcup

右下角杯子提供正常睡眠、保持运行但允许息屏、保持屏幕开启，以及持续保持或 30 分钟、1/2/4/8 小时定时。选择定时会立即开始保持运行；切换屏幕模式保留原结束时间。正常睡眠、定时到点或退出 Shifu 释放本次保持唤醒。

勾选「合盖时也保持运行」后，电脑合盖仍可继续工作。首次缺少权限时，在同一菜单确认授权，再由本人在 macOS 密码框操作。新增规则只允许两条固定命令：`pmset -a disablesleep 1` 和 `0`。如果本机已有这两条权限，会直接复用。授权脚本写入 `/etc/sudoers.d/shifu-steamcup`，不修改旧 Steamcup 的规则。

电池供电且电量不高于 10%，或 macOS 报告严重/临界发热时，15 秒内暂停合盖模式。低电量暂停后接上电源恢复；过热暂停后温度回到正常恢复。普通屏幕/闲置保持模式继续按选项执行。

## 恢复与接管

- 合盖状态记录在当前 Shifu 数据目录的 `power-lid.json`。只恢复 Shifu 自己开启的设置；若系统已经被 Steamcup 或其他程序设为合盖不睡，先关闭原有模式再开启 Shifu。
- 独立恢复任务是 `~/Library/LaunchAgents/<当前应用 bundle id>.power.recovery.plist`。每 60 秒检查一次，Shifu 异常退出、卡住且过热/低电、或定时已经到期时，恢复合盖睡眠。正常版和固定测试副本使用各自的应用身份与数据目录。
- 恢复任务直接调用当前 Shifu 的 `--power-watchdog <状态文件>`，无需打开 Steamcup，也不创建 Shifu 窗口。定时和安全检查运行在原生后台，隐藏窗口不影响它们。
- 取消勾选合盖模式会移除本应用的恢复任务。选择正常睡眠会结束当前保持，但保留本次运行内的合盖选项，下一次保持时继续按该选项执行。
- 如果系统拒绝恢复，保留持有状态并提示错误，后台继续恢复。手动恢复：`sudo pmset -a disablesleep 0`。

移植依据为本机 Steamcup 的 `Rules.swift`、`Power.swift`、`main.swift` 和首次授权脚本；保留其 [MIT 许可](third-party/Steamcup-LICENSE)。普通闲置睡眠仍使用原有 IOKit 断言；热状态通过 Foundation `NSProcessInfo` 读取。没有增加对 Steamcup 进程或安装路径的运行依赖。

本次实现围绕杯子内的睡眠、屏幕、定时、合盖及保护功能。Steamcup 独立应用的登录启动项、全局快捷键和命令行 `run` 未移植。
