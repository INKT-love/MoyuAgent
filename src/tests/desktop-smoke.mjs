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
      requireTwoFactor: false,
    };
    const endpoints = [
      { index: 0, name: "主线路", baseUrl: "https://inktandwkx.top" },
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
    };
    let callbackId = 1;
    const callbacks = new Map();
    let active;
    const loginResult = () => {
      snapshot = {
        ...snapshot,
        authenticated: true,
        user: { email: "test@example.com" },
      };
      return { user: snapshot.user, requiresTwoFactor: false, tempToken: null };
    };
    window.__TAURI_INTERNALS__ = {
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
              },
            };
            return snapshot.config;
          case "ack_stream":
            window.__smoke.acknowledgements.push(payload.sequence);
            return;
          case "start_stream": {
            if (!payload.requestId || !payload.prompt || !payload.onEvent)
              throw new Error("Invalid test stream arguments");
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
            active = () => {
              if (!done) {
                done = true;
                emit({ kind: "cancelled" });
                callbacks.get(payload.onEvent.id)?.({ index, end: true });
              }
            };
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
                  emit({ kind: "completed", sessionId: "test-session" });
                  callbacks.get(payload.onEvent.id)?.({ index, end: true });
                }
              }, 250);
            }
            return;
          }
          case "cancel_stream":
            active?.();
            return;
          default:
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
  await page.getByRole("button", { name: "设置", exact: true }).click();
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
  await expect(
    page.getByRole("button", { name: "停止任务" }),
  ).not.toBeVisible();
  await expect(page.locator(".message-content").last()).toContainText(
    "检查完成，配置有效。",
  );
  const acks = await page.evaluate(() => window.__smoke.acknowledgements);
  expect(acks.length).toBeGreaterThan(0);
  expect(acks.length).toBeLessThanOrEqual(3);
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
