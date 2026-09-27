import { chromium, expect } from "@playwright/test";
import { existsSync, mkdirSync } from "node:fs";

const edgePath = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const browser = await chromium.launch({
  headless: true,
  ...(existsSync(edgePath) ? { executablePath: edgePath } : {}),
});

try {
  const page = await browser.newPage({
    viewport: { width: 1280, height: 900 },
    reducedMotion: "reduce",
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    window.isTauri = true;
    window.__smoke = {
      commands: [],
      acknowledgements: [],
      streamRequests: [],
      requireTwoFactor: false,
    };
    const endpoints = [
      { index: 0, name: "主线路", baseUrl: "https://api.inktandwkx.top" },
      {
        index: 1,
        name: "备用线路(CF)",
        baseUrl: "https://inkaicf.flymiku.top",
      },
    ];
    let snapshot = {
      endpoint: endpoints[0],
      endpoints,
      authenticated: false,
      user: null,
      config: null,
      recentWorkspaces: [],
      pinnedWorkspaces: [],
      workspaceLabels: {},
    };
    let history = { activeId: "", conversations: [] };
    let callbackId = 1;
    const callbacks = new Map();
    const streams = new Map();
    const loginResult = () => {
      snapshot = {
        ...snapshot,
        authenticated: true,
        user: { email: "test@example.com" },
      };
      return { user: snapshot.user, requiresTwoFactor: false, tempToken: null };
    };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" } },
      transformCallback(callback) {
        const id = callbackId++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      async invoke(command, payload) {
        window.__smoke.commands.push(command);
        switch (command) {
          case "get_app_state":
            return snapshot;
          case "get_public_settings":
            return {
              loginAgreementRequired: false,
              loginAgreementUrl: null,
              registrationUrl: null,
            };
          case "login":
            return window.__smoke.requireTwoFactor
              ? {
                  user: null,
                  requiresTwoFactor: true,
                  tempToken: "test-temporary-token",
                }
              : loginResult();
          case "complete_two_factor":
            if (
              payload.tempToken !== "test-temporary-token" ||
              payload.totpCode !== "123456"
            )
              throw new Error("Invalid test two-factor arguments");
            return loginResult();
          case "logout":
            snapshot = { ...snapshot, authenticated: false, user: null };
            return;
          case "get_groups":
            return [{ id: 1, name: "默认分组", platform: "anthropic" }];
          case "get_group_models":
            return [{ id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" }];
          case "choose_workspace":
            snapshot = {
              ...snapshot,
              config: {
                ...(snapshot.config || {
                  groupId: 1,
                  groupName: "默认分组",
                  model: "claude-sonnet-4-6",
                  configPath: "D:/app-data/opencode.json",
                }),
                workspace: "D:/picked",
              },
            };
            return snapshot.config;
          case "reveal_workspace":
            return;
          case "workspace_action": {
            const path = payload.path || "";
            const same = (left, right) =>
              String(left).replace(/\\/g, "/").toLowerCase() ===
              String(right).replace(/\\/g, "/").toLowerCase();
            snapshot.recentWorkspaces = snapshot.recentWorkspaces || [];
            snapshot.pinnedWorkspaces = snapshot.pinnedWorkspaces || [];
            snapshot.workspaceLabels = snapshot.workspaceLabels || {};
            let switchTo = null;
            if (payload.action === "pin")
              snapshot.pinnedWorkspaces = [
                path,
                ...snapshot.pinnedWorkspaces.filter((item) => !same(item, path)),
              ];
            else if (payload.action === "unpin")
              snapshot.pinnedWorkspaces = snapshot.pinnedWorkspaces.filter(
                (item) => !same(item, path),
              );
            else if (payload.action === "rename") {
              const name = String(payload.name || "").trim();
              const labels = { ...snapshot.workspaceLabels };
              for (const key of Object.keys(labels))
                if (same(key, path)) delete labels[key];
              if (name) labels[path] = name;
              snapshot.workspaceLabels = labels;
            } else if (payload.action === "remove") {
              const wasCurrent = same(snapshot.config?.workspace || "", path);
              snapshot.recentWorkspaces = snapshot.recentWorkspaces.filter(
                (item) => !same(item, path),
              );
              snapshot.pinnedWorkspaces = snapshot.pinnedWorkspaces.filter(
                (item) => !same(item, path),
              );
              const labels = { ...snapshot.workspaceLabels };
              for (const key of Object.keys(labels))
                if (same(key, path)) delete labels[key];
              snapshot.workspaceLabels = labels;
              if (wasCurrent)
                switchTo =
                  snapshot.pinnedWorkspaces[0] ||
                  snapshot.recentWorkspaces[0] ||
                  "";
            } else if (payload.action === "create_worktree") {
              switchTo = `${path}-worktree`;
              snapshot.recentWorkspaces = [
                switchTo,
                ...snapshot.recentWorkspaces.filter((item) => !same(item, switchTo)),
              ];
            }
            return {
              recentWorkspaces: snapshot.recentWorkspaces,
              pinnedWorkspaces: snapshot.pinnedWorkspaces,
              workspaceLabels: snapshot.workspaceLabels,
              switchTo,
            };
          }
          case "list_plugins":
            return [];
          case "list_plugin_uis":
            return [];
          case "list_plugin_mixins":
            return [];
          case "list_conversations":
            return history;
          case "save_conversations":
            history = payload.history || { activeId: "", conversations: [] };
            return;
          case "install_plugin":
          case "set_plugin_enabled":
          case "uninstall_plugin":
            return [];
          case "switch_endpoint":
            snapshot = { ...snapshot, endpoint: endpoints[payload.index] };
            return snapshot.endpoint;
          case "configure":
            snapshot = {
              ...snapshot,
              config: {
                groupId: payload.groupId,
                groupName: "默认分组",
                model: payload.model,
                workspace: payload.workspace || "D:/workspace/project",
                configPath: "D:/app-data/opencode.json",
                reasoningEffort:
                  payload.reasoningEffort ||
                  snapshot.config?.reasoningEffort ||
                  "high",
                permissionMode:
                  payload.permissionMode ||
                  snapshot.config?.permissionMode ||
                  "assist",
              },
              recentWorkspaces: [
                payload.workspace || "D:/workspace/project",
                ...(snapshot.recentWorkspaces || []).filter(
                  (item) =>
                    item !== (payload.workspace || "D:/workspace/project"),
                ),
              ],
            };
            return snapshot.config;
          case "ack_stream":
            window.__smoke.acknowledgements.push(payload.sequence);
            return;
          case "start_stream": {
            if (!payload.requestId || !payload.prompt || !payload.onEvent)
              throw new Error("Invalid test stream arguments");
            window.__smoke.streamRequests.push({
              requestId: payload.requestId,
              prompt: payload.prompt,
              sessionId: payload.sessionId,
              workspace: payload.workspace,
            });
            let index = 0;
            let done = false;
            const emit = (event) =>
              callbacks.get(payload.onEvent.id)?.({
                index: index++,
                message: {
                  requestId: payload.requestId,
                  sequence: index,
                  ...event,
                },
              });
            const stop = () => {
              if (!done) {
                done = true;
                streams.delete(payload.requestId);
                emit({ kind: "cancelled" });
                callbacks.get(payload.onEvent.id)?.({ index, end: true });
              }
            };
            streams.set(payload.requestId, stop);
            emit({
              kind: "chunk",
              text: "已读取工作区。",
              sessionId: "test-session",
            });
            if (payload.prompt !== "取消测试") {
              setTimeout(() => {
                if (!done)
                  emit({ kind: "chunk", text: "\n正在检查项目配置。" });
              }, 20);
              setTimeout(() => {
                if (!done)
                  emit({ kind: "chunk", text: "\n检查完成，配置有效。" });
              }, 50);
              setTimeout(() => {
                if (!done) {
                  done = true;
                  streams.delete(payload.requestId);
                  emit({ kind: "completed", sessionId: "test-session" });
                  callbacks.get(payload.onEvent.id)?.({ index, end: true });
                }
              }, 250);
            }
            return;
          }
          case "cancel_stream":
            streams.get(payload.requestId)?.();
            return;
          case "plugin:event|listen":
            return callbackId++;
          case "plugin:event|unlisten":
            return;
          default:
            if (typeof command === "string" && command.startsWith("plugin:window|")) {
              if (command.endsWith("is_maximized")) return false;
              return;
            }
            throw new Error(`Unexpected IPC command: ${command}`);
        }
      },
    };
  });
  await page.goto(process.env.MOYU_TEST_URL || "http://127.0.0.1:1421/");
  await page.getByLabel("邮箱", { exact: true }).fill("test@example.com");
  await page.getByLabel("密码", { exact: true }).fill("test-password");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(page.getByLabel("任务内容")).toBeEnabled();
  await page.getByRole("button", { name: "选择模型" }).click();
  await expect(page.getByRole("dialog", { name: "选择分组" })).toBeVisible();
  await expect(page.getByRole("button", { name: "默认分组" })).toBeVisible();
  await expect(page.locator(".settings-page")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator(".model-picker")).toHaveCount(0);
  await page.getByRole("button", { name: "选择推理强度" }).click();
  await expect(page.getByRole("dialog", { name: "推理强度" })).toBeVisible();
  await expect(page.getByRole("slider", { name: "推理强度" })).toBeVisible();
  await expect(page.locator(".settings-page")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator(".reasoning-picker")).toHaveCount(0);
  await page.getByRole("button", { name: "选择权限" }).click();
  await expect(
    page.getByRole("dialog", { name: "应如何批准墨羽操作？" }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: /^请求批准/ })).toBeVisible();
  await expect(page.locator(".settings-page")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator(".permission-picker")).toHaveCount(0);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("tab", { name: "线路" }).click();
  await page.getByLabel("API 线路").first().selectOption("1");
  await expect(
    page.getByText("https://inkaicf.flymiku.top", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "对话", exact: true }).click();
  await page.getByLabel("任务内容").fill("检查项目配置");
  await page.getByRole("button", { name: "发送任务" }).click();
  await expect(page.locator(".message-content").last()).toContainText(
    "已读取工作区。",
  );
  await expect(page.getByRole("button", { name: "停止任务" })).toBeVisible();
  await expect(page.locator(".new-chat-button")).toBeEnabled();
  await page.locator(".new-chat-button").click();
  await expect(page.locator(".topbar-title strong")).toHaveText("新任务");
  await expect(page.getByRole("button", { name: "发送任务" })).toBeVisible();
  await page.getByLabel("任务内容").fill("并行任务");
  await page.getByRole("button", { name: "发送任务" }).click();
  await expect(page.getByRole("button", { name: "停止任务" })).toBeVisible();
  const streamPayloads = await page.evaluate(
    () => window.__smoke.streamRequests,
  );
  expect(streamPayloads.length).toBeGreaterThanOrEqual(2);
  expect(streamPayloads.at(-1).workspace).toBe("D:/workspace/project");
  await page.getByRole("button", { name: "检查项目配置", exact: true }).click();
  await expect(page.getByText("检查项目配置").first()).toBeVisible();
  await expect(page.locator(".assistant-message .message-content").last()).toContainText(
    "已读取工作区。",
  );
  await expect(page.locator(".message-cancelled")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "停止任务" }),
  ).not.toBeVisible();
  await expect(page.locator(".message-content").last()).toContainText(
    "检查完成，配置有效。",
  );
  const workspaceRow = page.getByRole("button", { name: "project", exact: true });
  const sessionOpen = page.getByRole("button", { name: "检查项目配置", exact: true });
  await expect(workspaceRow).toHaveAttribute("aria-expanded", "true");
  await expect(sessionOpen).toBeVisible();
  await workspaceRow.click();
  await expect(workspaceRow).toHaveAttribute("aria-expanded", "false");
  await expect(sessionOpen).toHaveCount(0);
  await workspaceRow.click();
  await expect(workspaceRow).toHaveAttribute("aria-expanded", "true");
  await expect(sessionOpen).toBeVisible();
  const projectWrap = page.locator(".workspace-row-wrap").filter({ has: workspaceRow });
  await projectWrap.hover();
  await expect(projectWrap.getByRole("button", { name: "更多选项" })).toBeVisible();
  await expect(projectWrap.getByRole("button", { name: "添加新会话" })).toBeVisible();
  await projectWrap.getByRole("button", { name: "更多选项" }).click();
  await expect(page.getByRole("menuitem", { name: "置顶" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "编辑" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "在资源管理器中打开" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "创建永久工作树" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "归档聊天" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "移除项目" })).toBeVisible();
  await page.getByRole("menuitem", { name: "置顶" }).click();
  await expect(workspaceRow).toHaveAttribute("aria-expanded", "true");
  await projectWrap.hover();
  await projectWrap.getByRole("button", { name: "更多选项" }).click();
  await expect(page.getByRole("menuitem", { name: "取消置顶" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".project-menu")).toHaveCount(0);
  await projectWrap.getByRole("button", { name: "添加新会话" }).click();
  await expect(page.locator(".topbar-title strong")).toHaveText("新任务");
  await expect(workspaceRow).toHaveAttribute("aria-expanded", "true");
  const sessionRow = page.locator(".session-row").filter({ hasText: "检查项目配置" });
  await sessionRow.hover();
  await expect(sessionRow.getByRole("button", { name: "置顶" })).toBeVisible();
  await expect(sessionRow.getByRole("button", { name: "归档" })).toBeVisible();
  await sessionRow.getByRole("button", { name: "置顶" }).click();
  await sessionRow.hover();
  await expect(sessionRow.getByRole("button", { name: "取消置顶" })).toBeVisible();
  await sessionRow.getByRole("button", { name: "归档" }).click();
  await expect(sessionOpen).toHaveCount(0);
  const acks = await page.evaluate(() => window.__smoke.acknowledgements);
  const streamCount = await page.evaluate(
    () => window.__smoke.streamRequests.length,
  );
  expect(acks.length).toBeGreaterThan(0);
  expect(acks.length).toBeLessThanOrEqual(streamCount * 3);
  await page.getByLabel("任务内容").fill("取消测试");
  await page.getByRole("button", { name: "发送任务" }).click();
  await page.getByRole("button", { name: "停止任务" }).click();
  await expect(page.locator(".message-cancelled")).toHaveText("已停止");
  await expect(page.locator(".stream-cursor")).toHaveCount(0);
  mkdirSync("node_modules/.cache/qa", { recursive: true });
  await page.screenshot({
    path: "node_modules/.cache/qa/mock-desktop-stream.png",
  });
  await page.getByRole("button", { name: "退出登录" }).click();
  await page.evaluate(() => {
    window.__smoke.requireTwoFactor = true;
  });
  await page.getByLabel("密码", { exact: true }).fill("test-password");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("动态验证码", { exact: true }).fill("123456");
  await page.getByRole("button", { name: "验证并登录", exact: true }).click();
  await expect(page.getByLabel("任务内容")).toBeEnabled();
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  expect(errors).toEqual([]);
  console.log(
    "PASS: login, automatic configuration, endpoint switch, streaming, acknowledgements, cancellation, two-factor login, mobile overflow, console errors",
  );
} finally {
  await browser.close();
}
