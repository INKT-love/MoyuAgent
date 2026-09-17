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
    const groups = [{ id: 1, name: "Anthropic", platform: "anthropic" }, { id: 2, name: "GPT-Pro", platform: "openai" }];
    const models = [
      { id: "codex-auto-review", name: "Codex Auto Review" },
      { id: "gpt-5.6-sol", name: "GPT 5.6 Sol" },
      { id: "gpt-5.6-terra", name: "GPT 5.6 Terra" },
      ...Array.from({ length: 6 }, (_, index) => ({ id: `test-model-${index}`, name: `Test Model ${index}` })),
    ];
    const state = { endpoint: endpoints[0], endpoints, authenticated: false, user: null, config: { groupId: 2, groupName: "GPT-Pro", model: "gpt-5.6-sol", workspace: "D:\\workspace", configPath: "D:\\appdata\\opencode.json" } };
    const testState = { configureRequests: [], modelRequests: 0 };
    window.__MOYU_TEST__ = testState;
    let active;
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback(callback) { const id = next++; callbacks.set(id, callback); return id; },
      unregisterCallback(id) { callbacks.delete(id); },
      async invoke(command, args = {}) {
        if (command === "get_app_state") return structuredClone({ ...state, recentWorkspaces: state.recentWorkspaces || [] });
        if (command === "get_public_settings") return { loginAgreementRequired: true, loginAgreementUrl: "https://inktandwkx.top/login", registrationUrl: null };
        if (command === "switch_endpoint") { state.endpoint = endpoints[args.index]; return state.endpoint; }
        if (command === "login") { state.authenticated = true; state.user = { email: "qa@example.test" }; return { user: state.user, requiresTwoFactor: false, tempToken: null }; }
        if (command === "get_groups") return structuredClone(groups);
        if (command === "get_group_models") { testState.modelRequests++; await new Promise(resolve => setTimeout(resolve, 80)); return structuredClone(models); }
        if (command === "configure") {
          testState.configureRequests.push(structuredClone(args));
          const groupId = args.groupId ?? state.config.groupId;
          state.config = { groupId, groupName: groups.find(group => group.id === groupId).name, model: args.model ?? state.config.model, workspace: args.workspace || "D:\\workspace", configPath: "D:\\appdata\\opencode.json" };
          return structuredClone(state.config);
        }
        if (command === "choose_workspace") { state.config = { ...state.config, workspace: "D:\\picked" }; return structuredClone(state.config); }
        if (command === "reveal_workspace") return;
        if (command === "list_plugins") return [{ id: "concise", name: "简洁模式", description: "缩短回复", version: "0.1.0", enabled: false, bundled: true, hasModule: false, hasInstructions: true, hasUi: false }, { id: "snake", name: "贪吃蛇", description: "主界面测试", version: "0.1.0", enabled: false, bundled: true, hasModule: false, hasInstructions: false, hasUi: true }];
        if (command === "list_plugin_uis") return [];
        if (command === "list_plugin_mixins") return [
          { pluginId: "snake", pluginName: "贪吃蛇", target: "", select: null, at: "RETURN", priority: 1000, html: "<p>snake-mixin</p>", script: null },
          { pluginId: "snake", pluginName: "贪吃蛇", target: "", select: ".composer-context", at: "RETURN", priority: 1000, html: "<span>Mixin</span>", script: null },
          { pluginId: "lab", pluginName: "测试页", target: "", select: ".settings-page", at: "RETURN", priority: 1000, html: "<p>settings-mixin</p>", script: null },
        ];
        if (command === "install_plugin" || command === "set_plugin_enabled" || command === "uninstall_plugin") return [];
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
        if (command === "plugin:event|listen") return next++;
        if (command === "plugin:event|unlisten") return;
        if (typeof command === "string" && command.startsWith("plugin:window|")) {
          if (command.endsWith("is_maximized")) return false;
          return;
        }
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
  assert.ok(await page.locator("iframe[title='贪吃蛇']").isVisible(), "ui.main mixin must mount");
  assert.equal((await page.locator(".mixin-slot").innerText()).trim(), "Mixin");
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
  await page.getByRole("button", { name: "选择工作区" }).click();
  await page.locator(".composer-chip").filter({ hasText: "picked" }).waitFor();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("tab", { name: "模型" }).waitFor();
  const modelSelect = page.locator("#model");
  const expectModel = async (expected, message) => {
    await page.waitForFunction(() => {
      const select = document.querySelector("#model");
      return select && !select.disabled && select.options.length === 9;
    });
    assert.equal(await modelSelect.inputValue(), expected, message);
    assert.equal(await page.locator("#group").inputValue(), "2", "The saved non-first group must remain selected");
  };
  await expectModel("gpt-5.6-sol", "The saved non-first model must be selected after asynchronous catalog loading");
  assert.equal(await modelSelect.evaluate(node => node.tagName), "SELECT");
  await modelSelect.selectOption("gpt-5.6-terra");
  await page.getByRole("button", { name: "一键配置", exact: true }).click();
  await page.getByText("配置已写入", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.__MOYU_TEST__.configureRequests.at(-1)), { groupId: 2, model: "gpt-5.6-terra", workspace: "D:\\picked" }, "Saving must send the selected model and preserve the group and workspace");
  await expectModel("gpt-5.6-terra", "Saving must keep the selected non-first model visible");
  await page.locator(".settings-summary").getByText("gpt-5.6-terra", { exact: true }).waitFor();
  await page.getByRole("button", { name: "对话", exact: true }).click();
  await prompt.waitFor();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expectModel("gpt-5.6-terra", "Returning to settings must restore the saved non-first model");
  await page.getByRole("tab", { name: "测试页" }).click();
  assert.ok(await page.locator("iframe[title='测试页']").isVisible(), "ui.settings mixin must mount a settings page");
  await page.getByRole("tab", { name: "模型" }).click();
  await expectModel("gpt-5.6-terra", "Returning to the model tab must restore the saved non-first model");
  const modelRequests = await page.evaluate(() => window.__MOYU_TEST__.modelRequests);
  await page.getByRole("button", { name: "刷新用户分组", exact: true }).click();
  await page.waitForFunction(previous => window.__MOYU_TEST__.modelRequests > previous, modelRequests);
  await expectModel("gpt-5.6-terra", "Refreshing groups and models must keep the saved selections");
  const modelFilter = page.getByPlaceholder("筛选模型名称或 ID");
  await modelFilter.fill("Sol");
  await page.waitForFunction(() => document.querySelector("#model").options.length === 2);
  assert.equal(await modelSelect.inputValue(), "gpt-5.6-terra", "Filtering must preserve the selected model even when it does not match the query");
  await modelSelect.selectOption("gpt-5.6-sol");
  await page.waitForFunction(() => document.querySelector("#model").options.length === 1);
  assert.equal(await modelSelect.inputValue(), "gpt-5.6-sol", "Selecting a filtered result must update the visible selection");
  await modelFilter.fill("");
  await expectModel("gpt-5.6-sol", "Clearing the model filter must preserve the selected non-first model");
  await page.getByRole("button", { name: "一键配置", exact: true }).click();
  await page.getByText("配置已写入", { exact: true }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.__MOYU_TEST__.configureRequests.at(-1)), { groupId: 2, model: "gpt-5.6-sol", workspace: "D:\\picked" }, "Saving a filtered selection must send the displayed model");
  await expectModel("gpt-5.6-sol", "The filtered model selection must remain visible after saving");
  await page.getByRole("tab", { name: "插件" }).click();
  await page.getByText("简洁模式").waitFor();
  await page.getByRole("tab", { name: "线路" }).click();
  const expectLine = async (index, url) => {
    await page.getByText(url, { exact: true }).waitFor();
    await page.waitForFunction((value) => [...document.querySelectorAll('select[aria-label="API 线路"]')].every(select => select.value === value), String(index));
  };
  await page.getByLabel("API 线路").last().selectOption("1");
  await expectLine(1, "https://inkaicf.flymiku.top");
  await page.getByLabel("API 线路").last().selectOption("0");
  await expectLine(0, "https://inktandwkx.top");
  await page.getByLabel("API 线路").last().selectOption("1");
  await expectLine(1, "https://inkaicf.flymiku.top");
  await page.screenshot({ path: "artifacts/settings-desktop.png", fullPage: true });
  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 });
    await page.waitForFunction(() => document.querySelector(".sidebar").getBoundingClientRect().right <= 0);
    await page.screenshot({ path: `artifacts/settings-${width}.png`, fullPage: true });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `Overflow at width ${width}`);
  }
  assert.deepEqual(errors, []);
  console.log("PASS: desktop/mobile layout; mock IPC login, asynchronous saved model selection, save payload, settings navigation, catalog refresh, model filtering, endpoint switch, live chunks, completion and cancellation; no browser errors.");
} catch (error) {
  console.error("Browser errors:", errors);
  const page = browser.contexts()[0]?.pages()[0];
  if (page) console.error("Page text:", (await page.locator("body").innerText()).slice(0, 4000));
  throw error;
} finally { await browser.close(); }
