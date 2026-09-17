# 入门

## 最小目录

```text
my-plugin/
  plugin.json     必填
```

按需再加：

```text
  backend.wasm            拦截任意后端命令，见 WASM 后端
  AGENTS.md               追加到 Agent 提示词
  index.mjs               OpenCode 插件
  opencode.patch.json     合并进 OpenCode 配置
  page.html / badge.html  界面混入用的 HTML
  bind.js                 界面混入用的脚本
```

## plugin.json

```json
{
  "id": "my-tools",
  "name": "我的工具",
  "description": "一句话说明",
  "version": "0.1.0",
  "mixins": [
    { "html": "overlay.html" },
    { "select": ".composer-context", "html": "badge.html" },
    { "select": ".settings-page", "html": "page.html" }
  ]
}
```

| 字段 | 要求 |
| --- | --- |
| `id` | 必填。小写字母、数字、短横线，最长 63，须以字母或数字开头 |
| `name` | 必填。出现在插件列表和设置标签上 |
| `description` | 可选 |
| `version` | 可选，默认 `0.1.0` |
| `mixins` | 可选。若省略且目录里有 `ui.html`，会当作主窗口浮层 |

`html` / `script` 只能写**当前目录下的文件名**，不能带路径。单个 html/js ≤ 256 KiB，`backend.wasm` ≤ 8 MiB。

## 安装

1. 打开 Moyu Agent。
2. **设置 → 插件 → 安装插件**。
3. 选中含 `plugin.json` 的文件夹。
4. 勾选启用。

改提示词或 OpenCode 补丁后，**下一次发送任务**才会用新配置。改 `backend.wasm` 后，关掉再打开该插件（或重新安装）才会重新加载。

内置插件（随应用提供）不能卸载，只能停用。

## 内置示例

安装后可在插件列表里看到，用来对照写法：

| 名称 | 目录 id | 作用 |
| --- | --- | --- |
| 简洁模式 | `concise` | 缩短 Agent 回复 |
| 审查模式 | `reviewer` | 先审查再改代码 |
| 贪吃蛇 | `snake` | 主窗口浮层 + 输入栏徽章 |
| 测试页 | `lab` | 设置里多一页 |
| WASM探针 | `probe` | 设置页 + 拦截 `get_engine_status` |

## 限制

- 插件拿不到文件系统、终端、凭证库。
- 完整 HTML 页面跑在沙箱 iframe 里，通过 `window.Moyu` 调命令，见 [界面](ui.md)。
- 选择器不能包含 `<` 或 `javascript`，最长 120 个字符。
- 往界面插内容时，插在稳定容器上（见界面文档里的 class 表），不要拆掉原有按钮内部结构。
- WASM 不能改原命令参数后再执行原命令，见 [WASM 后端](wasm.md)。
- `opencode.patch.json` 不能覆盖 `provider`、`model`、`$schema`。
