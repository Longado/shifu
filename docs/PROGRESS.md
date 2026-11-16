# 进度页 / Progress page

进度页显示工作区项目的 `GOALS.md` 清单、可选的工作区待办，以及 WorkBuddy 任务。卡片默认收起；点标题展开，同一窗口内的数据刷新会保留展开状态。任务与 Agent 本身不受收起影响。

工作区根目录沿用 `SHIFU_GOALS_ROOT`，默认是 `~/Desktop/Workspace`。如果根目录中存在 `.data/tasks.db`，Shifu 使用系统 `sqlite3` 以只读方式查询；不会创建、迁移或修改数据库。不存在这个文件时跳过待办来源。

待办数据库需有以下列：

- `projects`：`id`、`name`
- `tasks`：`id`、`project_id`、`title`、`status`、`due_date`

`status` 使用 `todo`、`doing`、`blocked`、`done`、`cancelled`，`due_date` 是可空的 `YYYY-MM-DD` 日期。页面只列仍有未完成任务的项目；完成数包含这些项目已完成的任务，取消的任务不计入。项目按最近截止日排列，未设日期的项目按名字排在后面；项目内依次显示正在做、卡住、待做的任务，同一状态按截止日排列。

如果数据库无法查询，页面显示错误信息，不会把读取失败当作任务已清空。这个来源不提供新建或编辑待办的入口。

## English

The progress page displays project `GOALS.md` checklists, an optional workspace task list and WorkBuddy tasks. Cards start collapsed. Click a heading to expand it; data refreshes preserve expanded cards within the same window. Collapsing a card does not stop its tasks or agents.

The workspace root uses the existing `SHIFU_GOALS_ROOT` setting, defaulting to `~/Desktop/Workspace`. When `.data/tasks.db` exists under that root, Shifu queries it read-only through the system `sqlite3`. It does not create, migrate or modify the database. A missing database simply skips this source.

The database needs these columns:

- `projects`: `id`, `name`
- `tasks`: `id`, `project_id`, `title`, `status`, `due_date`

Statuses are `todo`, `doing`, `blocked`, `done` and `cancelled`. `due_date` is a nullable `YYYY-MM-DD` date. Only projects with open work appear; their completed tasks contribute to the completion count and cancelled tasks are excluded. Projects are ordered by the nearest deadline, with undated projects listed alphabetically afterward. Within each project, doing tasks precede blocked and todo tasks, ordered by deadline within each status.

Query failures are shown as errors instead of an empty task list. This source does not provide task creation or editing.
