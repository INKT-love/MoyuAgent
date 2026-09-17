# 命令参考

`Moyu.invoke("命令名", { 参数 })` 和 `backend.wasm` 的 `command` 用同一套名字。参数是 camelCase。没传的字段不会出现在 wasm 的 `args` 里。

## 账户与线路

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `get_public_settings` | 无 | 登录页协议与注册链接 |
| `login` | `email`, `password`, `acceptedAgreement` | 登录 |
| `complete_two_factor` | `tempToken`, `totpCode` | 两步验证 |
| `logout` | 无 | 登出 |
| `get_app_state` | 无 | 当前线路、是否登录、模型配置、最近工作区 |
| `switch_endpoint` | `index`（`0` 主线路，`1` 备用） | 切换线路 |

## 模型与工作区

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `get_groups` | 无 | 可用模型分组 |
| `get_group_models` | `groupId` | 分组下的模型 |
| `configure` | `groupId`, `model`, `workspace` | 保存模型与工作区 |
| `choose_workspace` | 无 | 弹出目录选择，返回路径 |
| `reveal_workspace` | 无 | 在资源管理器中打开当前工作区 |

## 对话

| 命令 | 参数 | 做什么 |
| --- | --- | --- |
| `start_stream` | `requestId`, `prompt`, `sessionId`, `onEvent` | 开始一次 Agent 任务 |
| `ack_stream` | `requestId`, `sequence` | 流式确认 |
| `cancel_stream` | `requestId` | 停止生成 |
| `get_engine_status` | 无 | 引擎是否可用、进行中的请求数 |

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
