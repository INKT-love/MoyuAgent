# WASM 后端

在插件根目录放 `backend.wasm`，导出函数 `before`。每一次桌面端后端调用都会先问你的模块，按**命令名**决定要不要拦。

没有 `before` 导出的 wasm 会被跳过。模块不能访问磁盘、网络和环境变量。

## 输入

```json
{
  "command": "start_stream",
  "args": { "prompt": "...", "requestId": "..." }
}
```

`command` 是命令名，`args` 是这次调用的参数对象。完整名单见 [命令参考](commands.md)。

## 输出

| 你返回 | 结果 |
| --- | --- |
| `{}` | 放行，执行原来的后端 |
| `{ "cancel": true, "error": "原因" }` | 调用失败，界面收到这句错误 |
| `{ "result": 任意JSON }` | 不执行原来的后端，直接把 `result` 当作成功返回值 |

你**不能**改参数后再执行原来的命令。要改 Agent 怎么说话，用 [改 Agent](opencode.md)。

多个插件都启用时，列表里靠前的先跑；一旦有人 `cancel` 或 `result`，后面的不再处理这次调用。你的 wasm 如果抛错，这次会跳过你、仍执行原来的后端。

## 用 Rust 写

仓库里的探针源码在 `wasm-plugins/probe/`，可以当模板。

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    command: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Default, Serialize)]
struct Output {
    #[serde(skip_serializing_if = "is_false")]
    cancel: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[plugin_fn]
pub fn before(Json(input): Json<Input>) -> FnResult<Json<Output>> {
    if input.command == "get_engine_status" {
        return Ok(Json(Output {
            result: Some(serde_json::json!({
                "probe": "wasm-ok",
                "plugin": "probe"
            })),
            ..Output::default()
        }));
    }
    Ok(Json(Output::default()))
}
```

```toml
[package]
name = "my-plugin-wasm"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

```powershell
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
```

把生成的 `.wasm` 改名为 `backend.wasm`，和 `plugin.json` 放在同一目录。单文件不超过 8 MiB。

也可用其它语言的 [Extism PDK](https://extism.org)，只要导出 `before`，JSON 形状一致。

## 怎么验收

启用「WASM探针」后打开对应设置页，点「探测 WASM」，应看到 `"probe": "wasm-ok"`。它只替换 `get_engine_status`，不会动登录和发送。

替换 `get_app_state`、`login`、`start_stream` 时，必须返回界面期望的完整结构，否则页面会坏。
