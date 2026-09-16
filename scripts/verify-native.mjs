import { chromium } from "@playwright/test";
import assert from "node:assert/strict";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
try {
  const page = browser.contexts().flatMap(context => context.pages()).find(page => page.url().includes("127.0.0.1:1420"));
  assert.ok(page, "Start the debug desktop app with WebView2 remote debugging on port 9223");
  const result = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const initial = await invoke("get_app_state");
    const health = await invoke("get_engine_status");
    if (!initial.authenticated) await invoke("switch_endpoint", { index: 0 });
    const settings = await invoke("get_public_settings");
    const current = await invoke("get_app_state");
    return { authenticated: current.authenticated, configured: !!current.config, health, currentEndpoint: current.endpoint.index, settings, forbiddenSecretAccess: await invoke("plugin:store|get", { rid: 0, key: "token" }).then(() => false, error => /not allowed|permission|forbidden/i.test(String(error))) };
  });
  assert.equal(result.health.executableAvailable, true);
  assert.equal(result.forbiddenSecretAccess, true);
  assert.equal(result.settings.loginAgreementRequired, true);
  assert.ok(result.settings.agreementDocuments.length > 0);
  if (!result.authenticated) {
    await page.reload();
    await page.getByRole("button", { name: "服务协议", exact: true }).waitFor();
    await page.getByRole("button", { name: "服务协议", exact: true }).click();
    await page.getByRole("dialog").waitFor();
    await page.screenshot({ path: "artifacts/native-agreement.png" });
    await page.getByRole("button", { name: "关闭协议" }).click();
    await page.screenshot({ path: "artifacts/native-login.png" });
  }
  console.log(JSON.stringify({ nativeIpc: "PASS", authenticated: result.authenticated, configured: result.configured, executableHealth: result.health, selectedEndpoint: result.currentEndpoint, secureFrontendBoundary: result.forbiddenSecretAccess, agreementDocuments: result.settings.agreementDocuments.length }));
} finally { await browser.close(); }
