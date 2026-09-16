import { chromium } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import assert from "node:assert/strict";

await mkdir("artifacts", { recursive: true });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  page.on("pageerror", error => errors.push(error.message));
  await page.addInitScript(() => {
    const callbacks = new Map(); let next = 1;
    const endpoints = [{ index: 0, name: "主线路", baseUrl: "https://inktandwkx.top" }, { index: 1, name: "备用线路(CF)", baseUrl: "https://inkaicf.flymiku.top" }];
    const state = { endpoint: endpoints[0], endpoints, authenticated: false, user: null, config: null };
    let active;
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = {
      transformCallback(callback) { const id = next++; callbacks.set(id, callback); return id; },
      unregisterCallback(id) { callbacks.delete(id); },
      async invoke(command, args = {}) {
        if (command === "get_app_state") return structuredClone(state);
        if (command === "get_public_settings") return { loginAgreementRequired: true, loginAgreementUrl: "https://inktandwkx.top/login", registrationUrl: null };
        if (command === "switch_endpoint") { state.endpoint = endpoints[args.index]; return state.endpoint; }
        if (command === "login") { state.authenticated = true; state.user = { email: "qa@example.test" }; return { user: state.user, requiresTwoFactor: false, tempToken: null }; }
        if (command === "get_groups") return [{ id: 1, name: "Anthropic", platform: "anthropic" }];
        if (command === "configure") { state.config = { groupId: 1, groupName: "Anthropic", model: "claude-sonnet-4-6", workspace: "D:\\workspace", configPath: "D:\\appdata\\opencode.json" }; return state.config; }
        if (command === "get_engine_status") return { executableAvailable: true, activeRequests: active ? 1 : 0 };
        if (command === "ack_stream") return;
        if (command === "logout") { state.authenticated = false; state.user = null; return; }
        if (command === "start_stream") {
          const callback = callbacks.get(args.onEvent.id); let sequence = 0; let index = 0;
          const emit = payload => callback({ index: index++, message: { requestId: args.requestId, sequence: ++sequence, ...payload } });
          const end = kind => { clearInterval(active?.timer); active = undefined; emit({ kind, sessionId: "ses_test" }); callback({ end: true, index }); };
          let count = 0;
          active = { end, timer: setInterval(() => { emit({ kind: "chunk", text: `chunk ${++count} `, sessionId: "ses_test" }); if (count === 35) end("completed"); }, 12) };
          return;
        }
        if (command === "cancel_stream") { active?.end("cancelled"); return; }
        throw new Error(`Unimplemented test command: ${command}`);
      },
    };
  });
  await page.goto("http://127.0.0.1:1420/");
  await page.getByRole("heading", { name: "登录 Moyu Agent" }).waitFor();
  await page.screenshot({ path: "artifacts/login-desktop.png", fullPage: true });
  await page.getByLabel("邮箱", { exact: true }).fill("qa@example.test");
  await page.getByLabel("密码", { exact: true }).fill("local-test-only");
  await page.getByRole("checkbox").check();
  await page.getByRole("button", { name: "登录", exact: true }).click();
  const prompt = page.locator("textarea");
  await prompt.waitFor();
  await prompt.fill("Stream verification");
  await page.getByRole("button", { name: "发送任务" }).click();
  await page.getByText(/chunk 1 /).first().waitFor();
  assert.ok(await page.getByRole("button", { name: "停止任务" }).isVisible(), "First chunk must be visible while still loading");
  await page.getByText(/chunk 35/).waitFor();
  await page.getByRole("button", { name: "发送任务" }).waitFor();
  await page.screenshot({ path: "artifacts/chat-desktop.png", fullPage: true });
  await prompt.fill("Cancel verification");
  await page.getByRole("button", { name: "发送任务" }).click();
  await page.getByRole("button", { name: "停止任务" }).click();
  await page.getByRole("button", { name: "发送任务" }).waitFor();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByLabel("API 线路").last().selectOption("1");
  await page.getByText("https://inkaicf.flymiku.top", { exact: true }).waitFor();
  await page.waitForFunction(() => [...document.querySelectorAll('select[aria-label="API 线路"]')].every(select => select.value === "1"));
  await page.screenshot({ path: "artifacts/settings-desktop.png", fullPage: true });
  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 });
    await page.waitForFunction(() => document.querySelector(".sidebar").getBoundingClientRect().right <= 0);
    await page.screenshot({ path: `artifacts/settings-${width}.png`, fullPage: true });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `Overflow at width ${width}`);
  }
  assert.deepEqual(errors, []);
  console.log("PASS: desktop/mobile layout; mock IPC login, configuration, endpoint switch, live chunks, completion and cancellation; no browser errors.");
} finally { await browser.close(); }
