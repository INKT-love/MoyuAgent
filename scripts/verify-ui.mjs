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
    const endpoints = [{ index: 0, name: "主线路", baseUrl: "https://api.inktandwkx.top" }, { index: 1, name: "备用线路(CF)", baseUrl: "https://inkaicf.flymiku.top" }];
    const groups = [{ id: 1, name: "Anthropic", platform: "anthropic" }, { id: 2, name: "GPT-Pro", platform: "openai" }, { id: 8, name: "Gemini", platform: "gemini" }];
    const models = [
      { id: "codex-auto-review", name: "Codex Auto Review" },
      { id: "gpt-5.6-sol", name: "GPT 5.6 Sol" },
      { id: "gpt-5.6-terra", name: "GPT 5.6 Terra" },
      ...Array.from({ length: 6 }, (_, index) => ({ id: `test-model-${index}`, name: `Test Model ${index}` })),
    ];
    const state = { endpoint: endpoints[0], endpoints, authenticated: false, user: null, config: { groupId: 2, groupName: "GPT-Pro", model: "gpt-5.6-sol", workspace: "D:\\workspace", configPath: "D:\\appdata\\opencode.json", reasoningEffort: "high" }, recentWorkspaces: ["D:\\workspace"], pinnedWorkspaces: [], workspaceLabels: {} };
    let history = {
      activeId: "conv-restore",
      conversations: [{
        id: "conv-restore",
        title: "已保存的任务",
        messages: [
          { id: "m1", role: "user", text: "先前的提问" },
          { id: "m2", role: "assistant", text: "先前的回复", state: "completed" },
        ],
        workspace: "D:\\workspace",
        draft: "",
      }],
    };
    const testState = { configureRequests: [], groupRequests: 0, modelRequests: 0, history, workspaceActions: [], streamRequests: [] };
    window.__MOYU_TEST__ = testState;
    const streams = new Map();
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback(callback) { const id = next++; callbacks.set(id, callback); return id; },
      unregisterCallback(id) { callbacks.delete(id); },
      async invoke(command, args = {}) {
        if (command === "get_app_state") return structuredClone({ ...state, recentWorkspaces: state.recentWorkspaces || [], pinnedWorkspaces: state.pinnedWorkspaces || [], workspaceLabels: state.workspaceLabels || {} });
        if (command === "get_public_settings") return { loginAgreementRequired: true, loginAgreementUrl: "https://inktandwkx.top/login", registrationUrl: null };
        if (command === "switch_endpoint") { state.endpoint = endpoints[args.index]; return state.endpoint; }
        if (command === "login") { state.authenticated = true; state.user = { email: "qa@example.test" }; return { user: state.user, requiresTwoFactor: false, tempToken: null }; }
        if (command === "get_groups") { testState.groupRequests++; return structuredClone(groups); }
        if (command === "get_group_models") { testState.modelRequests++; await new Promise(resolve => setTimeout(resolve, 80)); return structuredClone(models); }
        if (command === "configure") {
          testState.configureRequests.push(structuredClone(args));
          const groupId = args.groupId ?? state.config.groupId;
          state.config = { groupId, groupName: groups.find(group => group.id === groupId).name, model: args.model ?? state.config.model, workspace: args.workspace || "D:\\workspace", configPath: "D:\\appdata\\opencode.json", reasoningEffort: args.reasoningEffort ?? state.config.reasoningEffort ?? "high", permissionMode: args.permissionMode ?? state.config.permissionMode ?? "assist" };
          return structuredClone(state.config);
        }
        if (command === "choose_workspace") { state.config = { ...state.config, workspace: "D:\\picked" }; state.recentWorkspaces = ["D:\\picked", ...(state.recentWorkspaces || []).filter(path => path !== "D:\\picked")]; return structuredClone(state.config); }
        if (command === "reveal_workspace") return;
        if (command === "workspace_action") {
          const path = args.path || "";
          const same = (left, right) => left.replace(/\\/g, "/").toLowerCase() === right.replace(/\\/g, "/").toLowerCase();
          state.recentWorkspaces = state.recentWorkspaces || [];
          state.pinnedWorkspaces = state.pinnedWorkspaces || [];
          state.workspaceLabels = state.workspaceLabels || {};
          let switchTo = null;
          if (args.action === "pin") state.pinnedWorkspaces = [path, ...state.pinnedWorkspaces.filter(item => !same(item, path))];
          else if (args.action === "unpin") state.pinnedWorkspaces = state.pinnedWorkspaces.filter(item => !same(item, path));
          else if (args.action === "rename") {
            const name = String(args.name || "").trim();
            for (const key of Object.keys(state.workspaceLabels)) if (same(key, path)) delete state.workspaceLabels[key];
            if (name) state.workspaceLabels[path] = name;
          } else if (args.action === "remove") {
            const wasCurrent = same(state.config?.workspace || "", path);
            state.recentWorkspaces = state.recentWorkspaces.filter(item => !same(item, path));
            state.pinnedWorkspaces = state.pinnedWorkspaces.filter(item => !same(item, path));
            for (const key of Object.keys(state.workspaceLabels)) if (same(key, path)) delete state.workspaceLabels[key];
            if (wasCurrent) switchTo = state.pinnedWorkspaces[0] || state.recentWorkspaces[0] || "";
          } else if (args.action === "create_worktree") {
            switchTo = `${path}-worktree`;
            state.recentWorkspaces = [switchTo, ...state.recentWorkspaces.filter(item => !same(item, switchTo))];
          }
          testState.workspaceActions = [...(testState.workspaceActions || []), { action: args.action, path, name: args.name, switchTo }];
          return structuredClone({ recentWorkspaces: state.recentWorkspaces, pinnedWorkspaces: state.pinnedWorkspaces, workspaceLabels: state.workspaceLabels, switchTo });
        }
        if (command === "list_plugins") return [{ id: "concise", name: "简洁模式", description: "缩短回复", version: "0.1.0", enabled: false, bundled: true, hasModule: false, hasInstructions: true, hasUi: false }, { id: "snake", name: "贪吃蛇", description: "主界面测试", version: "0.1.0", enabled: false, bundled: true, hasModule: false, hasInstructions: false, hasUi: true }];
        if (command === "list_plugin_uis") return [];
        if (command === "list_plugin_mixins") return [
          { pluginId: "snake", pluginName: "贪吃蛇", target: "", select: null, at: "RETURN", priority: 1000, html: "<p>snake-mixin</p>", script: null },
          { pluginId: "snake", pluginName: "贪吃蛇", target: "", select: ".composer-context", at: "RETURN", priority: 1000, html: "<span>Mixin</span>", script: null },
          { pluginId: "lab", pluginName: "测试页", target: "", select: ".settings-page", at: "RETURN", priority: 1000, html: "<p>settings-mixin</p>", script: null },
        ];
        if (command === "install_plugin" || command === "set_plugin_enabled" || command === "uninstall_plugin") return [];
        if (command === "get_engine_status") return { executableAvailable: true, activeRequests: streams.size };
        if (command === "ack_stream") return;
        if (command === "list_conversations") return structuredClone(history);
        if (command === "save_conversations") {
          history = structuredClone(args.history || { activeId: "", conversations: [] });
          testState.history = history;
          return;
        }
        if (command === "logout") { state.authenticated = false; state.user = null; return; }
        if (command === "start_stream") {
          testState.streamRequests = [...testState.streamRequests, { requestId: args.requestId, prompt: args.prompt, sessionId: args.sessionId, workspace: args.workspace }];
          const callback = callbacks.get(args.onEvent.id); let sequence = 0; let index = 0;
          const emit = payload => callback({ index: index++, message: { requestId: args.requestId, sequence: ++sequence, ...payload } });
          const end = kind => {
            const current = streams.get(args.requestId);
            if (current) clearInterval(current.timer);
            streams.delete(args.requestId);
            emit({ kind, sessionId: "ses_test" });
            callback({ end: true, index });
          };
          let count = 0;
          streams.set(args.requestId, { end, timer: setInterval(() => { emit({ kind: "chunk", text: `chunk ${++count} `, sessionId: "ses_test" }); if (count === 80) end("completed"); }, 40) });
          return;
        }
        if (command === "cancel_stream") { streams.get(args.requestId)?.end("cancelled"); return; }
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
  await page.getByRole("heading", { name: "登录墨羽AGENT" }).waitFor();
  assert.equal((await page.locator(".brand").innerText()).replace(/\s+/g, ""), "墨羽AGENT", "Sidebar title must be 墨羽AGENT");
  assert.ok(await page.locator(".login-icon img[src='/moyu.png']").count(), "Login mark must use the brand avatar");
  await page.screenshot({ path: "artifacts/login-desktop.png", fullPage: true });
  await page.getByLabel("邮箱", { exact: true }).fill("qa@example.test");
  await page.getByLabel("密码", { exact: true }).fill("local-test-only");
  await page.getByRole("checkbox").check();
  await page.getByRole("button", { name: "登录", exact: true }).click();
  const prompt = page.locator("textarea");
  await prompt.waitFor();
  await page.getByRole("button", { name: "已保存的任务" }).waitFor();
  await page.getByText("先前的回复").waitFor();
  assert.ok(await page.locator("iframe[title='贪吃蛇']").isVisible(), "ui.main mixin must mount");
  assert.equal((await page.locator(".mixin-slot").innerText()).trim(), "Mixin");
  await prompt.fill("Stream verification");
  await page.getByRole("button", { name: "发送任务" }).click();
  await page.getByText(/chunk 1 /).first().waitFor();
  assert.equal(await page.locator(".message-avatar").count(), 0, "Chat messages must not show avatars");
  assert.equal(await page.locator(".message-meta").count(), 0, "Chat messages must not show 你 / 墨羽 labels");
  const userAlign = await page.locator(".user-message").evaluateAll(nodes => nodes.map(node => getComputedStyle(node).justifyContent));
  assert.ok(userAlign.length >= 2 && userAlign.every(value => value === "flex-end"), "User prompts must sit on the right");
  assert.ok(await page.getByRole("button", { name: "停止任务" }).isVisible(), "First chunk must be visible while still loading");
  assert.ok(await page.locator(".new-chat-button").isEnabled(), "New task must stay available while another conversation streams");
  assert.ok(await page.getByRole("button", { name: "已保存的任务" }).isEnabled(), "Saved task must stay clickable while streaming");
  await page.locator(".new-chat-button").click();
  await page.locator(".topbar-title strong").filter({ hasText: "新任务" }).waitFor();
  assert.ok(await page.getByRole("button", { name: "发送任务" }).isVisible(), "Switching away from a running chat must not keep the stop button");
  await prompt.fill("Parallel verification");
  await page.getByRole("button", { name: "发送任务" }).click();
  await page.getByText(/chunk 1 /).first().waitFor();
  const streamRequests = await page.evaluate(() => window.__MOYU_TEST__.streamRequests);
  assert.ok(streamRequests.length >= 2, "A second conversation must start without cancelling the first");
  assert.equal(streamRequests.at(-1).workspace, "D:\\workspace", "start_stream must pass the conversation workspace");
  await page.getByRole("button", { name: "已保存的任务" }).click();
  await page.getByText("先前的回复").waitFor();
  await page.getByText("Stream verification").waitFor();
  assert.equal(await page.locator(".message-cancelled").count(), 0, "Switching chats must not cancel the other stream");
  await page.getByRole("button", { name: "Parallel verification" }).waitFor();
  await page.getByText(/chunk 35/).waitFor();
  await page.getByRole("button", { name: "发送任务" }).waitFor();
  await page.waitForFunction(() => window.__MOYU_TEST__.history?.conversations?.some(item =>
    item.messages?.some(message => message.role === "user" && message.text === "Stream verification")
  ));
  const modelChip = await page.locator(".model-chip").boundingBox();
  const sendButton = await page.locator(".send-button").boundingBox();
  assert.ok(modelChip && sendButton, "Model chip and send button must be visible");
  assert.ok(modelChip.x + modelChip.width <= sendButton.x + 2, "Model chip must sit to the left of the send button");
  assert.ok(modelChip.x > sendButton.x - 240, "Model chip must sit next to the send button, not in the workspace cluster");
  await page.getByRole("button", { name: "选择模型" }).click();
  await page.getByRole("dialog", { name: "选择分组" }).waitFor();
  assert.equal(await page.locator(".settings-page").count(), 0, "Model chip must not open Settings");
  await page.getByRole("button", { name: "GPT-Pro", exact: true }).click();
  await page.getByRole("dialog", { name: "选择模型" }).waitFor();
  await page.getByRole("button", { name: "GPT 5.6 Sol", exact: true }).waitFor();
  await page.getByText("正在加载模型").waitFor({ state: "hidden" });
  await page.screenshot({ path: "artifacts/model-picker-desktop.png" });
  await page.getByRole("button", { name: "GPT 5.6 Sol", exact: true }).click();
  await page.getByRole("dialog", { name: "推理强度" }).waitFor();
  await page.getByRole("button", { name: "极高", exact: true }).click();
  await page.locator(".model-chip").getByText("5.6 Sol 极高", { exact: true }).waitFor();
  assert.equal(await page.locator(".model-picker").count(), 0, "Picker must close after choosing effort");
  assert.equal(await page.locator(".settings-page").count(), 0, "Choosing a model must stay on the chat view");
  assert.deepEqual(await page.evaluate(() => window.__MOYU_TEST__.configureRequests.at(-1)), { groupId: 2, model: "gpt-5.6-sol", workspace: "D:\\workspace", reasoningEffort: "xhigh" }, "Picker must persist the group, model, and reasoning effort");
  await page.getByRole("button", { name: "选择权限" }).click();
  await page.getByRole("dialog", { name: "应如何批准墨羽操作？" }).waitFor();
  assert.equal(await page.locator(".settings-page").count(), 0, "Permission chip must not open Settings");
  assert.ok(await page.getByRole("button", { name: /^请求批准/ }).isVisible());
  assert.ok(await page.getByRole("button", { name: /^帮我批准/ }).isVisible());
  assert.ok(await page.getByRole("button", { name: /^完全访问权限/ }).isVisible());
  await page.screenshot({ path: "artifacts/permission-picker-desktop.png" });
  await page.getByRole("button", { name: /^完全访问权限/ }).click();
  await page.locator(".permission-chip").getByText("完全访问", { exact: true }).waitFor();
  assert.equal(await page.locator(".permission-picker").count(), 0, "Permission picker must close after choosing a mode");
  assert.deepEqual(await page.evaluate(() => window.__MOYU_TEST__.configureRequests.at(-1)), { groupId: 2, model: "gpt-5.6-sol", workspace: "D:\\workspace", permissionMode: "full" }, "Permission chip must persist the selected mode");
  await page.screenshot({ path: "artifacts/chat-desktop.png", fullPage: true });
  const projectRow = page.getByRole("button", { name: "workspace", exact: true });
  const projectWrap = page.locator(".workspace-row-wrap").filter({ has: projectRow });
  await projectWrap.hover();
  assert.ok(await projectWrap.getByRole("button", { name: "更多选项" }).isVisible(), "Project hover must show more options");
  assert.ok(await projectWrap.getByRole("button", { name: "添加新会话" }).isVisible(), "Project hover must show add session");
  await projectWrap.getByRole("button", { name: "更多选项" }).click();
  const projectMenu = page.locator(".project-menu");
  await projectMenu.waitFor();
  assert.deepEqual(
    (await projectMenu.getByRole("menuitem").allTextContents()).map(text => text.trim()),
    ["置顶", "编辑", "在资源管理器中打开", "创建永久工作树", "归档聊天", "移除项目"],
  );
  await page.screenshot({ path: "artifacts/workspace-menu-desktop.png" });
  await projectMenu.getByRole("menuitem", { name: "置顶" }).click();
  await page.waitForFunction(() => (window.__MOYU_TEST__.workspaceActions || []).some(item => item.action === "pin"));
  await projectWrap.hover();
  await projectWrap.getByRole("button", { name: "添加新会话" }).click();
  await page.locator(".topbar-title strong").filter({ hasText: "新任务" }).waitFor();
  await page.getByRole("button", { name: "已保存的任务" }).click();
  await page.getByText("先前的回复").waitFor();
  for (const width of [390, 360]) {
    await page.setViewportSize({ width, height: 844 });
    await page.waitForFunction(() => document.querySelector(".sidebar").getBoundingClientRect().right <= 0);
    const mobileAlign = await page.locator(".user-message").evaluateAll(nodes => nodes.map(node => getComputedStyle(node).justifyContent));
    assert.ok(mobileAlign.every(value => value === "flex-end"), `User prompts must stay right-aligned at width ${width}`);
    assert.equal(await page.locator(".message-avatar").count(), 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `Chat overflow at width ${width}`);
    await page.screenshot({ path: `artifacts/chat-${width}.png` });
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "选择模型" }).click();
  await page.getByRole("dialog", { name: "选择分组" }).waitFor();
  assert.equal(await page.locator(".settings-page").count(), 0, "Mobile model chip must not open Settings");
  await page.screenshot({ path: "artifacts/model-picker-390.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "选择权限" }).click();
  await page.getByRole("dialog", { name: "应如何批准墨羽操作？" }).waitFor();
  assert.equal(await page.locator(".settings-page").count(), 0, "Mobile permission chip must not open Settings");
  await page.screenshot({ path: "artifacts/permission-picker-390.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "打开导航" }).click();
  await page.waitForFunction(() => {
    const sidebar = document.querySelector(".sidebar");
    if (!sidebar?.classList.contains("is-open")) return false;
    const rect = sidebar.getBoundingClientRect();
    return rect.left >= -1 && rect.right >= 200;
  });
  assert.ok(await page.getByRole("button", { name: "更多选项" }).first().isVisible(), "Mobile project actions must stay visible");
  assert.ok(await page.getByRole("button", { name: "添加新会话" }).first().isVisible(), "Mobile add-session must stay visible");
  await page.getByRole("button", { name: "更多选项" }).first().click();
  await page.locator(".project-menu").waitFor();
  await page.screenshot({ path: "artifacts/workspace-menu-390.png" });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "对话", exact: true }).click();
  await page.waitForFunction(() => document.querySelector(".sidebar").getBoundingClientRect().right <= 0);
  await page.setViewportSize({ width: 1440, height: 960 });
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
  assert.equal(await page.locator("#group option").count(), 3, "Settings must list every Sub2API group including Gemini");
  assert.ok(await page.locator("#group option", { hasText: "Gemini" }).count(), "Gemini groups must remain selectable");
  const groupRequests = await page.evaluate(() => window.__MOYU_TEST__.groupRequests);
  const modelRequests = await page.evaluate(() => window.__MOYU_TEST__.modelRequests);
  await page.getByRole("button", { name: "刷新用户分组", exact: true }).click();
  await page.waitForFunction(previous => window.__MOYU_TEST__.groupRequests > previous, groupRequests);
  assert.equal(await page.evaluate(() => window.__MOYU_TEST__.modelRequests), modelRequests, "Refreshing groups must not refetch models");
  await page.getByRole("button", { name: "刷新模型列表", exact: true }).click();
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
  await expectLine(0, "https://api.inktandwkx.top");
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
  console.log("PASS: desktop/mobile layout; mock IPC login, composer model picker, asynchronous saved model selection, save payload, settings navigation, catalog refresh, model filtering, endpoint switch, live chunks, completion and cancellation; no browser errors.");
} catch (error) {
  console.error("Browser errors:", errors);
  const page = browser.contexts()[0]?.pages()[0];
  if (page) console.error("Page text:", (await page.locator("body").innerText()).slice(0, 4000));
  throw error;
} finally { await browser.close(); }
