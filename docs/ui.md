# 界面

对着页面上已有的 CSS class 插入 HTML 或脚本，不必登记注入点。

## mixins 怎么写

```json
{ "select": ".composer-context", "at": "RETURN", "html": "badge.html" }
```

| 写法 | 效果 |
| --- | --- |
| 只有 `html`，不写 `select` | 完整页面浮在主窗口右上角 |
| `"select": ".某个class"` | 插入匹配到的节点 |
| `"select": ".settings-page"` | 设置里多出一页，标签用插件的 `name` |
| `"at": "HEAD"` | 插到该节点最前面 |
| `"at": "RETURN"` 或省略 | 插到该节点最后面 |
| `"script": "bind.js"` | 对匹配节点执行一次脚本 |

`html` 和 `script` 可以同时写。

## 可以插到哪

| 选择器 | 位置 |
| --- | --- |
| `.app-shell` | 整窗 |
| `.sidebar-nav` | 侧栏「对话 / 设置」 |
| `.history-section` | 最近任务 |
| `.sidebar-bottom` | 侧栏底部（工作区 / 账号） |
| `.topbar` | 顶栏 |
| `.topbar-actions` | 顶栏右侧 |
| `.chat-pane` | 对话区 |
| `.empty-chat` | 空对话欢迎区 |
| `.transcript` | 消息列表 |
| `.message-tools` | 助手消息的复制 / 重试 |
| `.composer-area` | 输入区 |
| `.composer-toolbar` | 输入栏整行 |
| `.composer-context` | 输入栏左侧（工作区、模型） |
| `.send-button` | 发送按钮 |
| `.composer-footnote` | 输入栏下方状态 |
| `.login-page` | 登录页 |
| `.login-content` | 登录表单区域 |
| `.settings-page` | 设置页（完整 HTML 会变成新标签） |
| `.settings-tabs` | 设置标签栏 |
| `.settings-card` | 设置卡片 |
| `.settings-footer` | 设置页底部 |

界面变化后会再插入一次，已经插过的不会重复。

## 片段 vs 完整页

**片段**（徽章等）直接插进节点：

```html
<span style="font-size:10px;color:#276554;padding:0 8px;height:26px;display:inline-flex;align-items:center;border-radius:999px;background:#eef3f0;">WASM</span>
```

```json
{ "select": ".composer-context", "html": "badge.html" }
```

**完整文档**（以 `<!doctype` 或 `<html` 开头）放进 iframe。

浮层：

```json
{ "html": "game.html" }
```

设置页：

```json
{ "select": ".settings-page", "html": "page.html" }
```

## 选择器脚本

`bind.js` 必须是函数表达式。非法脚本会被忽略，页面保持原样。

```js
(el) => {
  el.addEventListener("click", (event) => {
    event.stopImmediatePropagation();
  });
}
```

```json
{ "select": ".send-button", "script": "bind.js" }
```

每个匹配节点只跑一次。脚本里不要直接调桌面端命令，完整页请用下面的 `Moyu`。

## 完整页里的 Moyu

完整 HTML 会被注入 `window.Moyu`：

```js
const state = await Moyu.state();
const status = await Moyu.invoke("get_engine_status");
await Moyu.invoke("login", {
  email: "you@example.com",
  password: "secret",
  acceptedAgreement: true
});
```

| 方法 | 说明 |
| --- | --- |
| `Moyu.state()` | 读取当前应用状态（同 `get_app_state`） |
| `Moyu.invoke(命令名, 参数对象)` | 调用后端命令。第二个参数必须是对象 |

命令名和参数见 [命令参考](commands.md)。这些调用同样会被其他插件的 `backend.wasm` 拦截。
