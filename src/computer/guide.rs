use serde_json::json;

pub const GUIDE: &str = r#"Shifu computer 使用指南

优先使用应用已有的 API/CLI；必须操作界面时才使用本工具。先运行 capabilities、list-apps，再用 list-windows --app <应用> 列出窗口。
以下子命令均通过 shifu computer 调用，例如：
  shifu computer capabilities --json
  shifu computer list-windows --app com.apple.TextEdit --json
  shifu computer get-app-state --app com.apple.TextEdit --window-id <刚返回的ID> --json
用 get-app-state --app <应用> --window-id <ID> 明确选择应用和窗口；多窗口时不要依赖默认窗口，也可按返回顺序使用 --window-index。
先读取目标窗口，之后使用动作返回的新观察选择下一步；延迟、导航、切窗或目标不确定时重新 get-app-state。不要猜索引或使用其他对话的索引。
动作回读的 snapshot.treeDiff:true 表示差量：~ 是变化行，- 是删除行，No change 表示控件树未变化；未列出的控件沿用 baselineSnapshotId 对应状态，索引不重排。没有 treeDiff 时按完整树读取。
需要完整树时使用 get-app-state 或动作 --full-tree；新增、移动、结构变化或缺少可靠基线时自动返回完整树。--match/--limit 筛选之后的下一动作也返回完整树。文字/文件条件始终用完整观察判断，不拿差量中的缺失判消失。
默认由 SHIFU_ID 隔离操作上下文；外部终端或并发调用应各自指定 --session <名字>。

每次只执行一个改变界面的动作，等待它的 JSON 回执和新观察后再决定下一步；不要批量盲目操作或自动重试。
用 --wait-for / --wait-for-gone 和 --timeout-ms 明确界面后置条件；它们只重复观察，不会重做动作。
文件任务用绝对路径的 --expect-path / --expect-missing-path 核对结果；路径存在性不证明内容正确或业务完成。
ok:true 不等于任务完成；检查 action.verification、result.postcondition 和实际回读，taskCompletion:not_checked 不能宣称完成。

截图保存在本地文件，响应的 result.screenshot.path 给出 screenshot 路径，而不是图片内容。
调用 agent 必须用自己的图片读取工具打开该路径并查看截图；仅拿到路径不代表已经看过，本命令不会替 agent 打开图片。
不需要截图时使用 --no-screenshot；截图像素坐标须按返回的 scale 换算。

上传文件时，先核对本地绝对路径及目标站点。打开选择框后重新 list-windows，用选择框自己的窗口 ID 读取；不能用底下网页的控件树代表选择框。需要前台时须在已有授权内使用 --restore-window。
标准 macOS 选择框可用 CmdOrCtrl+Shift+G 打开路径框；发现新路径窗口后读取其聚焦输入框，用 set-value --value-stdin 写完整路径并核对回读。Return 只确认路径，再读取原选择框，确认唯一选中的文件行与预期文件名一致且 Open 可用，才点 Open 一次。
最后回到网页核对附件文件名、大小及上传完成/错误状态。选择框关闭不等于上传完成，更不等于申请提交。按钮疑似未生效时先检查新窗口和附件；没有完成状态不要盲点或重传。网页读取不到上传状态时保留未知，请本人核查。

ok:false 时先读 error、action 和 postcondition；postcondition_timeout、断线或未验证结果都不能宣称成功。
observation_failed 可能发生在动作已经执行之后；若存在 action 回执，不要重复该动作，先重新 get-app-state 观察，再决定恢复步骤。
没有 action 回执也不证明动作没有发生；断线或结果未知时同样先观察。result.recovery 是下一步建议，不会自动执行恢复或重做动作。
snapshot.foreground 记录回读采样时的前台：target_app、other_app 或 unknown。临时编辑框消失且前台已切换时，应重新观察和选定目标，再决定下一步；不要把已投递按键当成编辑成功。该状态不是前台占用承诺，foreground-fallback 结束后仍会恢复原应用。
权限错误先运行 permissions，并请本人在系统设置中授权；不要代点授权框。必要时用 stop 停止当前辅助进程。

默认后台操作。只有本人明确允许接管前台时才使用 --restore-window，并在任务授权范围内操作。
--foreground-fallback 只用于带等待条件的 get-app-state，也只应在前台切换已获明确授权时使用；它只补充读取，不重做动作。
提交表单、发送消息、购买、删除及处理敏感授权须在当前任务已有的明确授权范围内；结果不确定时先观察，不盲目重试。"#;

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => println!("{GUIDE}"),
        [flag] if flag == "--json" => println!("{}", serde_json::to_string_pretty(&json!({"version": 1, "guide": GUIDE})).map_err(|e| e.to_string())?),
        _ => return Err("computer guide 只接受 --json".into()),
    }
    Ok(())
}
