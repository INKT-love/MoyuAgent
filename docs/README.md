# 编写 Moyu Agent 插件

你不需要改 Moyu Agent 源码。做一个含 `plugin.json` 的文件夹，在桌面端 **设置 → 插件 → 安装插件** 里选中并启用即可。

| 你想做的事 | 看这篇 |
| --- | --- |
| 目录怎么放、`plugin.json` 怎么写、怎么安装 | [入门](plugins.md) |
| 往界面上插按钮、浮层、设置页 | [界面](ui.md) |
| 拦截或替换登录、发送等后端调用 | [WASM 后端](wasm.md) |
| 可以拦截 / 调用的命令名和参数 | [命令参考](commands.md) |
| 改 Agent 的提示词、工具、OpenCode 配置 | [改 Agent](opencode.md) |

对照用的内置插件：贪吃蛇（浮层）、测试页（设置页）、WASM探针（拦 `get_engine_status`）。

发布安装包时必须提升版本号，然后推送对应标签（例如 `v0.1.1`）。GitHub Actions 会构建 Windows NSIS 安装包并新建 Release。不要覆盖已有 Release 或替换其中的安装包。`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 三者版本必须一致。
