# Issue 跟踪：GitHub

本仓库的 issue 与 PRD 以 GitHub issue 形式记录，所有操作均通过 `gh` CLI 完成。

## 约定

- **创建 issue**：`gh issue create --title "..." --body "..."`。多行正文使用 heredoc。
- **查看 issue**：`gh issue view <number> --comments`，用 `jq` 过滤评论，并一并取回标签。
- **列出 issue**：`gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`，按需附加 `--label` 与 `--state` 过滤。
- **评论 issue**：`gh issue comment <number> --body "..."`
- **添加 / 移除标签**：`gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **关闭**：`gh issue close <number> --comment "..."`

仓库从 `git remote -v` 推断——本仓库远程为 `origin → https://github.com/Buktal/VaultOne.git`，在 clone 内运行 `gh` 时会自动识别。

## Pull request 作为 triage 入口

**PR 是否作为请求入口：否。** _（若本仓库把外部 PR 当作功能请求处理，可改为 `是`；`/triage` 会读取此标记。）_

设为 `是` 时，PR 与 issue 走相同的标签与状态流程，命令改用 `gh pr` 的等价形式：

- **查看 PR**：`gh pr view <number> --comments`，并用 `gh pr diff <number>` 查看 diff。
- **列出待 triage 的外部 PR**：`gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments`，仅保留 `authorAssociation` 为 `CONTRIBUTOR`、`FIRST_TIME_CONTRIBUTOR` 或 `NONE` 的（剔除 `OWNER`/`MEMBER`/`COLLABORATOR`）。
- **评论 / 打标签 / 关闭**：`gh pr comment`、`gh pr edit --add-label`/`--remove-label`、`gh pr close`。

GitHub 中 issue 与 PR 共用同一编号空间，因此裸 `#42` 可能是其中任意一个——先用 `gh pr view 42` 解析，失败则回退到 `gh issue view 42`。

## 当技能说「发布到 issue 跟踪」时

创建一个 GitHub issue。

## 当技能说「获取相关 ticket」时

运行 `gh issue view <number> --comments`。

## Wayfinding 操作

供 `/wayfinder` 使用。**map** 是单个 issue，其下挂 **child** issue 作为 ticket。

- **Map**：单个带 `wayfinder:map` 标签的 issue，正文承载 Notes / Decisions-so-far / Fog。`gh issue create --label wayfinder:map`。
- **Child ticket**：作为 GitHub 子 issue（sub-issue）挂到 map 下的 issue。若未启用 sub-issue，则把 child 加入 map 正文的 task list，并在 child 正文顶部写 `Part of #<map>`。标签：`wayfinder:<type>`（`research`/`prototype`/`grilling`/`task`）。被认领后，ticket 指派给驱动的开发者。
- **Blocking（阻塞）**：GitHub **原生 issue 依赖**——UI 可见的规范表示。用 `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>` 添加一条边，其中 `<blocker-db-id>` 是阻塞方的数字 **database id**（`gh api repos/<owner>/<repo>/issues/<n> --jq .id`，_不是_ `#number` 或 `node_id`）。GitHub 通过 `issue_dependencies_summary.blocked_by` 报告（仅含未关闭的阻塞——即实时门槛）。若依赖不可用，回退到 child 正文顶部的 `Blocked by: #<n>, #<n>` 行。当所有阻塞方都已关闭时，ticket 即解除阻塞。
- **Frontier 查询**：列出 map 下未关闭的 child（`gh issue list --state open`，限定到 map 的 sub-issue / task list），剔除任何仍有未关闭阻塞（`issue_dependencies_summary.blocked_by > 0`，或 `Blocked by` 行中存在未关闭 issue）或已指派 assignee 的项；按 map 顺序取第一个。
- **认领（Claim）**：`gh issue edit <n> --add-assignee @me`——这是本会话的第一次写入。
- **解决（Resolve）**：`gh issue comment <n> --body "<answer>"`，再 `gh issue close <n>`，最后把上下文指针（gist + 链接）追加到 map 的 Decisions-so-far。
