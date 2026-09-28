# Moyu Agent

Tauri v2 + SolidJS + Rust/Tokio desktop client for Sub2API, with a bundled OpenCode 1.18.31 engine.

## Run

Prerequisites: Node.js 22+, current stable Rust, and the [Tauri v2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci
npm run sidecar:prepare
npm run tauri -- dev
```

On this Windows workspace, a portable Microsoft compiler/SDK is already available in `.tools`. Start from PowerShell with:

```powershell
./scripts/dev.ps1
```

The script loads the local compiler environment and can reuse the existing Vite server. The browser preview is at `http://127.0.0.1:1420`; account operations and the agent require the desktop app. No real credentials are supplied in tests or previews.

Plugin developers: [docs/README.md](docs/README.md).

Production build, after preparing the sidecar for the current host:

```sh
npm run tauri -- build
```

Each published installer is a new GitHub Release with a new version number (`v0.1.1`, `v0.1.2`, …). Keep `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` in sync, then create a matching `vX.Y.Z` tag. Do not replace an existing Release or overwrite its installer.

On Windows, run this from a Visual Studio Developer PowerShell, or first dot-source `. ./.tools/env-msvc.ps1` in this workspace. macOS builds require Xcode Command Line Tools. Linux needs WebKitGTK 4.1 and an unlocked Secret Service keyring. Platform-specific sidecars are prepared for Windows x64, macOS x64/arm64, and Linux x64/arm64. This is a desktop application; the requested mobile entry attribute does not imply a working mobile OpenCode sidecar.

## Delivery Steps

| Step | Implementation | Verification |
| --- | --- | --- |
| 1 | `services/api_client.rs`, `services/auth.rs`, `services/config.rs`, `components/Settings.tsx` | Sign in with the account email/password, accept the service's actual agreement, then inspect Settings. Login automatically loads groups and configures the first available group. Switch between the two endpoints and restart the app to verify persistence. |
| 2 | `services/opencode.rs`, `resources/stream-bridge.mjs`, `scripts/prepare-sidecar.mjs` | `npm run test:sidecar` runs the real CLI against a local Anthropic SSE server and prints token deltas in the terminal. |
| 3 | `services/stream.rs`, Tauri `Channel<StreamEvent>` | `cargo test --manifest-path src-tauri/Cargo.toml --lib` covers bounded queues, ACK flow control, cancellation, terminal delivery and recovery after startup failure. |
| 4 | `App.tsx`, `lib/stream-controller.ts` | `npm test` covers immediate first-chunk rendering, 16ms batching, terminal flushing, duplicate filtering, ACK timing, timeouts and unlisten. `npm run test:ui` checks browser interaction and desktop/mobile screenshots using a clearly separate IPC fixture. |
| 5 | `services/opencode.rs`, `lib.rs` | Remove/rename the sidecar temporarily to observe a failed terminal, then restore it and send again. Stop a running response and verify loading ends. Suspend a consumer to exercise ACK timeout; stop output to exercise idle timeout. |

Real account login, remote API-key creation and paid inference require your account and available model quota. The repository does not contain account credentials. Tests use loopback HTTP fixtures or unauthenticated public settings.

## Streaming Architecture

```text
Persistent `opencode serve` (loopback, ephemeral port, basic auth)
  -> POST /session and POST /session/{id}/prompt_async
  -> GET /event SSE (`message.part.delta` / `session.idle` / `session.error`)
  -> tokio::sync::mpsc::channel(100)
  -> Tauri Channel, monotonic request sequence, <=128 unacknowledged events
  -> first chunk immediately; subsequent text coalesced for 16ms
  -> ACK every 100ms on a separate clock from rendering
  -> exactly one Completed / Failed / Cancelled terminal for each accepted stream
```

OpenCode starts with the signed-in, configured app and stays running. Each workspace has its own serve process; conversations in different sessions can prompt at the same time, and Stop cancels only that request. The same OpenCode session cannot run two prompts at once. Idle serves restart when `opencode.json`, plugins, the selected API line, or the group API key change; a busy serve keeps its current process so in-flight work is not aborted. Health is `GET /global/health` on the live servers, or a sidecar-exists check before one has started. Windows Job Objects and Unix process groups still cover each serve process tree. A task that has already received its prompt is not automatically replayed after a crash because it may have changed files or executed commands. Terminal delivery is best effort when a window is already closed; the dead frontend cannot receive events, but Rust still aborts the session and keeps the server for the next request.

An mpsc bound alone does not bound Tauri's IPC queue. The ACK window supplies end-to-end flow control. A reserved terminal slot bypasses a saturated ACK window, and frontend cancellation is independent of the SSE reader.

Limits: 64 KiB prompt, 1 MiB SSE event, 16 KiB forwarded chunk, 32 MiB total event bytes; 5s heartbeat, 30s ACK timeout, 180s idle timeout, 30-minute total runtime. The UI has its own 30s connection watchdog. Bounds produce a clear failure rather than indefinite loading or uncontrolled allocation.

## Authentication and Configuration

- Primary: `https://api.inktandwkx.top`; backup: `https://inkaicf.flymiku.top`.
- Sub2API account/configuration requests go through `ApiClient`. Connection failures, timeouts and temporary gateway failures switch the selected endpoint and retry once. Authentication errors do not retry. Redirects are disabled to keep credentials on the selected host.
- Verified account APIs: `/api/v1/settings/public`, `/api/v1/auth/login`, `/api/v1/auth/login/2fa`, `/api/v1/auth/me`, `/api/v1/auth/refresh`, `/api/v1/auth/logout`, `/api/v1/groups/available`, `/api/v1/keys`.
- Response codes support the deployment's numeric success and string error envelopes. Error responses do not echo raw request/response bodies into the UI.
- The key creation request uses one stable `Idempotency-Key` and a cryptographically random `custom_key` across retries. A pending record is saved in the OS vault before submission; ambiguous failures can be retried without creating unrelated duplicate keys. A 409 is reconciled against the same account/group/key.
- API keys are scoped by account ID and group ID. Switching accounts cannot silently reuse another account's local configuration. Existing keys are reused; logout removes access/refresh tokens, while generated group API keys stay in the OS vault for the next login. Revoke a key in Sub2API to invalidate it server-side.
- `tauri-plugin-store` saves endpoint, non-secret configuration metadata, the cached group/model catalog, and per-account chat history. The app writes `opencode.json` and the streaming plugin under `app.path().app_data_dir()`, and uses an app-owned default workspace unless you choose a directory.
- OpenCode configuration contains the `{env:SUB2API_API_KEY}` reference, never the actual key. Rust injects the selected group's key only into the persistent serve process environment. OpenCode connects to the currently selected model endpoint; an interrupted model generation fails explicitly and can be resent after switching endpoints.
- Settings lists every active Sub2API group. Groups and models are fetched on first use, then persisted locally; the model tab has refresh buttons for both. Anthropic/Claude groups use `@ai-sdk/anthropic`; other platforms use the OpenAI-compatible SDK. Defaults are `claude-sonnet-4-6` and `gpt-5.2` when the platform has no catalog yet.
- OpenCode can read, edit and execute commands in the configured workspace. The composer permission chip defaults to 帮我批准 (`external_directory` denied, internet allowed). 请求批准 also denies `webfetch` / `websearch`; 完全访问 allows internet and files outside the workspace. OpenCode retains its own local session database. The desktop conversation list is stored per account in `conversations.json`; recent workspaces stay in `settings.json`. Switching accounts does not show another user's chats.

## Tauri v2 Compatibility

`tauri-plugin-secure-store` was not published on crates.io at implementation time. With the user's approval, `src-tauri/plugins/secure-store` is a local plugin backed by the mature `keyring` crate: Windows Credential Manager, macOS Keychain and Linux Secret Service. Platform features are explicit so the crate cannot silently use its mock backend. Vault operations and store disk I/O run in `spawn_blocking`; secrets have no JavaScript plugin commands.

Tauri v2 replaced `tauri::api::path` with `Manager::path()` / `app.path()`. The business entry is `lib.rs`, including `#[cfg_attr(mobile, tauri::mobile_entry_point)]`; `main.rs` is only a launcher.

Tauri Channel has no public Event-style `unlisten()` method. The component calls its own `unlisten` cleanup, detaches `Channel.onmessage`, cancels timers and requests Rust cancellation. Dropping the Rust Channel sends its end marker so Tauri unregisters the JavaScript callback. This avoids using unsupported/private runtime APIs in application code.

`build.rs` generates app-command permissions, and `capabilities/default.json` explicitly permits every custom command. The frontend does not receive store, filesystem, shell or vault plugin permissions. Release CSP restricts connection destinations to Tauri IPC.

## Checks

```sh
npm run build
npm test
npm run test:sidecar
npm run test:ui
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
```

`test:ui` expects the local Vite server and Microsoft Edge. `src/tests/desktop-smoke.mjs` additionally tests 2FA; set `MOYU_TEST_URL=http://127.0.0.1:1420` when running it. The optional real-vault test creates and deletes a unique synthetic test credential:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib operating_system_vault_roundtrip -- --ignored
```

`scripts/verify-native.mjs` tests actual Windows Tauri IPC, engine health, denied direct store access, public settings, automatic failover and agreement display. It requires a debug app launched with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`. Keep that flag out of normal launches.

The CI workflow runs frontend build/tests and Rust tests across Windows, macOS and Linux. Cross-platform CI is provided but has not been executed from this local workspace. Windows native compilation and local tests are the verified platform here.
