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

Plugin authoring (no Agent code changes, CSS `select` mixins, OpenCode patches) is in [docs/plugins.md](docs/plugins.md).

Production build, after preparing the sidecar for the current host:

```sh
npm run tauri -- build
```

On Windows, run this from a Visual Studio Developer PowerShell, or first dot-source `. ./.tools/env-msvc.ps1` in this workspace. macOS builds require Xcode Command Line Tools. Linux needs WebKitGTK 4.1 and an unlocked Secret Service keyring. Platform-specific sidecars are prepared for Windows x64, macOS x64/arm64, and Linux x64/arm64. This is a desktop application; the requested mobile entry attribute does not imply a working mobile OpenCode sidecar.

## Delivery Steps

| Step | Implementation | Verification |
| --- | --- | --- |
| 1 | `services/api_client.rs`, `services/auth.rs`, `services/config.rs`, `components/Settings.tsx` | Sign in with the account email/password, accept the service's actual agreement, then inspect Settings. Login automatically loads groups and configures the first supported group. Switch between the two endpoints and restart the app to verify persistence. |
| 2 | `services/opencode.rs`, `resources/stream-bridge.mjs`, `scripts/prepare-sidecar.mjs` | `npm run test:sidecar` runs the real CLI against a local Anthropic SSE server and prints token deltas in the terminal. |
| 3 | `services/stream.rs`, Tauri `Channel<StreamEvent>` | `cargo test --manifest-path src-tauri/Cargo.toml --lib` covers bounded queues, ACK flow control, cancellation, terminal delivery and recovery after startup failure. |
| 4 | `App.tsx`, `lib/stream-controller.ts` | `npm test` covers immediate first-chunk rendering, 16ms batching, terminal flushing, duplicate filtering, ACK timing, timeouts and unlisten. `npm run test:ui` checks browser interaction and desktop/mobile screenshots using a clearly separate IPC fixture. |
| 5 | `services/opencode.rs`, `lib.rs` | Remove/rename the sidecar temporarily to observe a failed terminal, then restore it and send again. Stop a running response and verify loading ends. Suspend a consumer to exercise ACK timeout; stop output to exercise idle timeout. |

Real account login, remote API-key creation and paid inference require your account and available model quota. The repository does not contain account credentials. Tests use loopback HTTP fixtures or unauthenticated public settings.

## Streaming Architecture

```text
OpenCode provider SSE
  -> local plugin message.part.delta hook
  -> bounded OS stdout pipe (JSONL)
  -> Tokio AsyncBufReadExt, maximum 1 MiB per line
  -> tokio::sync::mpsc::channel(100)
  -> Tauri Channel, monotonic request sequence, <=128 unacknowledged events
  -> first chunk immediately; subsequent text coalesced for 16ms
  -> ACK every 100ms on a separate clock from rendering
  -> exactly one Completed / Failed / Cancelled terminal for each accepted stream
```

Stock `opencode run --format json` emits completed text parts, so it does not provide token-by-token output on its own. The shipped plugin forwards actual OpenCode text-delta events as JSONL. Rust suppresses the duplicate completed text. The bridge's synchronous pipe writes intentionally apply OS backpressure because OpenCode does not await plugin event promises. No global Tauri Event is used for conversation data.

An mpsc bound alone does not bound Tauri's IPC queue. The ACK window supplies end-to-end flow control. A reserved terminal slot bypasses a saturated ACK window, and frontend cancellation is independent of stdout reads. stdout, stderr, process exit, cancellation and the watchdog are serviced independently. stderr is continuously drained without logging credentials or prompt contents.

Limits: 64 KiB prompt, 1 MiB JSONL line, 16 KiB forwarded chunk, 32 MiB total CLI output; 5s heartbeat, 30s ACK timeout, 180s idle timeout, 30-minute total runtime. The UI has its own 30s connection watchdog. Bounds produce a clear failure rather than indefinite loading or uncontrolled allocation.

The startup health probe executes `opencode --version` with a 3s timeout and one automatic retry. Each request uses a fresh child, optionally resuming an OpenCode session. The process is killed/reaped on cancellation or failure; Windows Job Objects and Unix process groups cover descendants. A task that has already received its prompt is not automatically replayed after a crash because it may have changed files or executed commands. The next request starts a new process. Terminal delivery is best effort when a window is already closed; the dead frontend cannot receive events, but Rust still cancels and cleans up the child.

## Authentication and Configuration

- Primary: `https://inktandwkx.top`; backup: `https://inkaicf.flymiku.top`.
- Sub2API account/configuration requests go through `ApiClient`. Connection failures, timeouts and temporary gateway failures switch the selected endpoint and retry once. Authentication errors do not retry. Redirects are disabled to keep credentials on the selected host.
- Verified account APIs: `/api/v1/settings/public`, `/api/v1/auth/login`, `/api/v1/auth/login/2fa`, `/api/v1/auth/me`, `/api/v1/auth/refresh`, `/api/v1/auth/logout`, `/api/v1/groups/available`, `/api/v1/keys`.
- Response codes support the deployment's numeric success and string error envelopes. Error responses do not echo raw request/response bodies into the UI.
- The key creation request uses one stable `Idempotency-Key` and a cryptographically random `custom_key` across retries. A pending record is saved in the OS vault before submission; ambiguous failures can be retried without creating unrelated duplicate keys. A 409 is reconciled against the same account/group/key.
- API keys are scoped by account ID and group ID. Switching accounts cannot silently reuse another account's local configuration. Existing keys are reused; logout removes access/refresh tokens, while generated group API keys stay in the OS vault for the next login. Revoke a key in Sub2API to invalidate it server-side.
- `tauri-plugin-store` saves only endpoint and non-secret configuration metadata. The app writes `opencode.json` and the streaming plugin under `app.path().app_data_dir()`, and uses an app-owned default workspace unless you choose a directory.
- OpenCode configuration contains the `{env:SUB2API_API_KEY}` reference, never the actual key. Rust injects the selected group's key only into the child environment. OpenCode connects to the currently selected model endpoint; an interrupted model generation fails explicitly and can be resent after switching endpoints.
- Supported provider groups are Anthropic and OpenAI. The model name is editable in Settings because actual model availability and aliases depend on the account. Defaults are `claude-sonnet-4-6` and `gpt-5.2`; unsupported platform groups are not silently mapped to the wrong protocol.
- OpenCode can read, edit and execute commands in the configured workspace. Its external-directory permission is denied. OpenCode retains its own local session database; the current desktop recent-task list is in memory.

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

The CI workflow runs frontend build/tests and Rust tests across Windows, macOS and Linux. Cross-platform CI is provided but has not been executed from this local workspace. Windows native compilation and local tests are the verified platform here. During verification the primary hostname failed TLS negotiation on this machine while the backup responded successfully; this is an observation of the local network, not a global availability claim.
