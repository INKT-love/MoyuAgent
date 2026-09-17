# Moyu Agent 插件

写插件不必改 Agent 代码，也不必向宿主登记注入点。做一个含 `plugin.json` 的目录，在 **设置 → 插件** 里安装并启用即可。

内置示例在 `src-tauri/resources/harness/`：`concise`（提示词）、`reviewer`（提示词 + OpenCode 补丁）、`snake`（主界面浮层）、`lab`（设置页）。

## 目录

```text
my-plugin/
  plugin.json              必填
  AGENTS.md                可选，追加到 Agent 提示词
  index.mjs                可选，作为 OpenCode 插件加载
  opencode.patch.json      可选，合并进 opencode.json
  *.html / *.js            可选，给 mixins 引用
```

`id` 只能是小写字母、数字和短横线，最长 63 位，例如 `my-tools`。html / js 文件名只能是当前目录下的普通文件名，不能带路径。单个文件不超过 256 KiB。

## plugin.json

```json
{
  "id": "my-tools",
  "name": "我的工具",
  "description": "一句话说明",
  "version": "0.1.0",
  "mixins": [
    { "html": "overlay.html" },
    { "select": ".composer-context", "at": "RETURN", "html": "badge.html" },
    { "select": ".settings-page", "html": "page.html" },
    { "target": "chat.send", "at": "HEAD", "script": "send.js" }
  ]
}
```

`mixins` 可省略。没有 `mixins` 但目录里有 `ui.html` 时，会当作主窗口浮层。

## 界面：用 CSS 选择器，不要写注入点

| 写法 | 效果 |
| --- | --- |
| 只有 `html`，不写 `select` | 完整页面浮在主窗口右上角 |
| `"select": ".某个class"` | 插进页面上已有的节点 |
| `"select": ".settings-page"` | 在设置里多出一页，标签用插件的 `name` |
| `"at": "HEAD"` | 插到该节点最前面 |
| `"at": "RETURN"`（默认） | 插到该节点最后面 |

`select` 对着的是当前界面的 class，常见位置：

| 选择器 | 位置 |
| --- | --- |
| `.app-shell` | 整窗根节点 |
| `.sidebar-nav` | 侧栏「对话 / 设置」 |
| `.history-section` | 最近任务 |
| `.sidebar-bottom` | 侧栏底部工作区 / 账号 |
| `.topbar` | 顶栏 |
| `.topbar-actions` | 顶栏右侧 |
| `.chat-pane` | 对话区域 |
| `.empty-chat` | 空对话欢迎区 |
| `.transcript` | 消息列表 |
| `.message-tools` | 助手消息的复制 / 重试 |
| `.composer-area` | 输入区 |
| `.composer-toolbar` | 输入栏整行（含发送） |
| `.composer-context` | 输入栏左侧芯片（工作区、模型） |
| `.composer-footnote` | 输入栏下方状态 |
| `.login-page` | 登录页 |
| `.login-content` | 登录表单区域 |
| `.settings-page` | 设置页（完整 HTML 会变成新标签） |
| `.settings-tabs` | 设置标签栏 |
| `.settings-card` | 设置卡片 |
| `.settings-footer` | 设置页底部保存栏 |

片段 HTML（例如一枚徽章）会直接插进节点。以 `<!doctype` 或 `<html` 开头的完整文档会放进带 `sandbox="allow-scripts"` 的 iframe。

宿主会在界面变化后自动再插一次；已经插过的节点不会重复。

### 浮层示例

```json
{ "html": "game.html" }
```

### 输入栏徽章示例

`badge.html`：

```html
<span style="font-size:10px;color:#276554;padding:0 8px;height:26px;display:inline-flex;align-items:center;border-radius:999px;background:#eef3f0;">Mixin</span>
```

```json
{ "select": ".composer-context", "html": "badge.html" }
```

### 设置页示例

```json
{ "select": ".settings-page", "html": "page.html" }
```

`page.html` 写成完整 HTML 文档。启用后，设置栏会出现以插件 `name` 命名的标签。

## 行为：只有拦方法才写 target

函数不能用 CSS 选中。目前宿主会把发送任务交给 Mixin：

| target | 时机 |
| --- | --- |
| `chat.send` | 用户点发送或回车 |

`at`：

- `HEAD`：原方法之前。`ci.cancel(value)` 可以拦掉发送。
- `RETURN` / `TAIL`：原方法之后。
- `WRAP`：包在原方法外面，可改参数再调用。

`script` 必须是一个函数表达式，非法脚本会被忽略，原方法照常执行。

`send.js` 拦截示例：

```js
(ci) => {
  if (String(ci.args[0]).includes("secret")) ci.cancel();
}
```

`WRAP` 示例：

```js
(original, text) => original(`${text}\n\n请用中文简短回复。`)
```

```json
{ "target": "chat.send", "at": "WRAP", "script": "wrap.js" }
```

`HEAD` 里的 `ci`：

- `args`：可改。发送时第一项是当前输入文本。
- `cancel(value)`：不再调用原方法。
- `returnValue`：`RETURN` 时可改。

## 改 Agent 本身

这些文件不走 Mixin，启用插件后会编进 OpenCode 配置。

### AGENTS.md

追加到默认 Agent 提示词。例如缩短回复、先审查再改代码。

### index.mjs / index.js

当作 OpenCode 插件加载，可用 OpenCode 的 `config` / 工具 / 钩子。工作目录是插件自己的目录。

### opencode.patch.json

深合并进 `opencode.json`。不能改 `provider`、`model`、`$schema`。`plugin` 数组里的路径相对本插件目录。

## 安装

1. 桌面客户端打开 **设置 → 插件 → 安装插件**。
2. 选中含 `plugin.json` 的目录。
3. 勾选启用。改提示词或 OpenCode 补丁后，下一次任务会用新配置。

把目录放到 `src-tauri/resources/harness/<id>/` 再编译，会随应用内置，且不能从界面卸载。日常第三方插件用安装即可，不必改仓库。

## 限制

- 前端没有文件系统、Shell、凭证库权限。HTML / 脚本由 Rust 读出后交给界面。
- 插件脚本跑在页面里，可改你选中的 DOM；不要假设能调用任意 Tauri 命令。
- 选择器里不能带 `<` 或 `javascript`，长度不超过 120。
- Solid 会重绘自己的节点。插入点请挂在上面列出的稳定容器上，不要替换按钮内部结构。
