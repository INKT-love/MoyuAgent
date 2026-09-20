# 改 Agent

用来改对话时 Agent 的提示词、工具和 OpenCode 配置。不走界面 mixin，也不走 `backend.wasm`。

启用或停用带这些文件的插件后，**下一次发送**才生效。

## AGENTS.md

纯文本，会追加到默认 Agent 提示词后面。

适合：缩短回复、规定先审查再改代码、固定输出格式。

内置「简洁模式」「审查模式」就是这样写的。

## index.mjs / index.js

作为 OpenCode 插件加载，工作目录是你的插件目录。

可以注册 OpenCode 的配置钩子、工具和事件。不要打印用户密钥或完整 prompt。

## opencode.patch.json

合并进 OpenCode 配置。

- 可以改 `agent`、`plugin` 等字段。
- 不能覆盖 `provider`、`model`、`$schema`（模型和线路由用户在设置里选）。
- `plugin` 数组里的路径相对你的插件目录。

Agent 默认只能在当前工作区里读文件、改文件、执行命令，并可以使用互联网；工作区以外的目录默认禁止。输入栏权限芯片可改成「请求批准」（同时禁止上网与工作区外文件）或「完全访问」（允许上网和读写工作区外文件）。
