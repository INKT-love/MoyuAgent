# 命令参考

`Moyu.invoke("命令名", { 参数 })` 和 `backend.wasm` 的 `command` 用同一套名字。参数是 camelCase。没传的字段不会出现在 wasm 的 `args` 里。

## 账户与线路

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `get_public_settings` | 无 | 登录页协议与注册链接 |
| `login` | `email`, `password`, `acceptedAgreement` | 登录 |
| `complete_two_factor` | `tempToken`, `totpCode` | 两步验证 |
| `logout` | 无 | 登出 |
| `get_app_state` | 无 | 当前线路、是否登录、模型配置、最近工作区、置顶工作区、工作区别名 |
| `switch_endpoint` | `index`（`0` 主线路，`1` 备用） | 切换线路 |

## 模型与工作区

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `get_groups` | `refresh`（可选） | 可用模型分组。缺省读本地缓存；首次或 `refresh: true` 时从 Sub2API 拉取并保存 |
| `get_group_models` | `groupId`，`refresh`（可选） | 分组下的模型。同样是首次拉取后缓存，刷新才重新请求 |
| `configure` | `groupId`, `model`, `workspace`, `reasoningEffort`（可选，`low` / `medium` / `high` / `xhigh`），`permissionMode`（可选，`ask` / `assist` / `full`） | 保存分组、模型、推理强度、权限模式与工作区。省略 `reasoningEffort` / `permissionMode` 时保留已有值，缺省分别为 `high` 和 `assist` |
| `choose_workspace` | 无 | 弹出目录选择，返回路径 |
| `reveal_workspace` | `path`（可选） | 在资源管理器中打开指定目录；缺省为当前工作区 |
| `workspace_action` | `action`，`path`，`name`（重命名时） | 工作区置顶 / 取消置顶 / 重命名 / 打开 / 创建 Git worktree / 移除。不在命令内切换配置；需要换目录时通过 `switchTo` 交给前端 `configure` |

## 对话

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `start_stream` | `requestId`, `prompt`, `sessionId`，`workspace`（可选），`onEvent` | 开始一次 Agent 任务。可与其他会话并行；`workspace` 指定该会话工作区，省略则用当前配置。同一会话不能重复跑 |
| `ack_stream` | `requestId`, `sequence` | 流式确认 |
| `cancel_stream` | `requestId` | 只停止这一次生成，不影响其他会话 |
| `get_engine_status` | 无 | 引擎是否可用、所有工作区进行中的请求数。每个工作区常驻一个 OpenCode；空闲进程才在改模型/插件/线路时重启 |
| `list_conversations` | 无 | 读取当前账户保存在本地的对话。按账户隔离；未完成的流式回复会标成已停止 |
| `save_conversations` | `history`（`activeId`、`conversations`） | 把当前账户的对话写到本地 `conversations.json`。工作区列表仍在设置里，聊天记录单独存 |

`prompt` 最长 64 KiB。`onEvent` 是流式通道：不要对带通道的命令返回自定义 `result`，除非你不再需要流式输出。

## 插件管理

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `list_plugins` | 无 | 列出插件 |
| `install_plugin` | 无（会弹出目录框） | 安装插件 |
| `set_plugin_enabled` | `id`, `enabled` | 启用或停用 |
| `uninstall_plugin` | `id` | 卸载（内置插件不行） |
| `list_plugin_uis` | 无 | 已启用插件的界面 HTML |
| `list_plugin_mixins` | 无 | 已启用插件的 mixins |

拦截插件管理相关命令可能让设置页无法使用。

## 权限

你只能调用上表中的命令，不能直接读写磁盘、跑终端或读凭证。
