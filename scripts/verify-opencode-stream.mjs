import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const targets = { "win32-x64": "x86_64-pc-windows-msvc.exe", "darwin-arm64": "aarch64-apple-darwin", "darwin-x64": "x86_64-apple-darwin", "linux-x64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu" };
const binary = path.join(root, "src-tauri", "binaries", `opencode-${targets[`${process.platform}-${process.arch}`]}`);
const directory = await mkdtemp(path.join(tmpdir(), "moyu-stream-check-"));
const events = [];
let buffer = "";
let firstDeltaAt;
let providerCompletedAt;
let child;
const stderr = [];
const server = createServer(async (request, response) => {
  let input = "";
  for await (const chunk of request) input += chunk;
  const body = JSON.parse(input || "{}");
  if (!request.url.startsWith("/v1/messages")) { response.writeHead(404).end(); return; }
  const message = { id: "msg_fixture", type: "message", role: "assistant", content: [], model: "fixture-model", stop_reason: null, stop_sequence: null, usage: { input_tokens: 5, output_tokens: 0 } };
  if (!body.stream) {
    response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ ...message, stop_reason: "end_turn", content: [{ type: "text", text: "Fixture title" }] }));
    return;
  }
  response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" });
  const emit = (type, data) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`);
  emit("message_start", { message });
  emit("content_block_start", { index: 0, content_block: { type: "text", text: "" } });
  emit("content_block_delta", { index: 0, delta: { type: "text_delta", text: "First token. " } });
  await new Promise(resolve => setTimeout(resolve, 1500));
  emit("content_block_delta", { index: 0, delta: { type: "text_delta", text: "Last token." } });
  emit("content_block_stop", { index: 0 });
  emit("message_delta", { delta: { stop_reason: "end_turn", stop_sequence: null }, usage: { output_tokens: 8 } });
  emit("message_stop", {});
  providerCompletedAt = Date.now();
  response.end();
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const configPath = path.join(directory, "opencode.json");
await writeFile(configPath, JSON.stringify({
  plugin: [pathToFileURL(path.join(root, "src-tauri/resources/stream-bridge.mjs")).href],
  enabled_providers: ["fixture"],
  provider: { fixture: { npm: "@ai-sdk/anthropic", options: { baseURL: `http://127.0.0.1:${server.address().port}/v1`, apiKey: "local-test-only" }, models: { "fixture-model": { name: "Fixture", limit: { context: 128000, output: 8000 } } } } },
  model: "fixture/fixture-model", autoupdate: false, share: "disabled", permission: { "*": "deny" },
}));
child = spawn(binary, ["run", "--format", "json"], { cwd: directory, windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, OPENCODE_CONFIG: configPath, OPENCODE_DISABLE_DEFAULT_PLUGINS: "true", OPENCODE_DISABLE_AUTOUPDATE: "true", NO_COLOR: "1" } });
child.stdout.setEncoding("utf8");
child.stdout.on("data", data => {
  buffer += data;
  let index;
  while ((index = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
    if (!line.trim()) continue;
    const event = JSON.parse(line); events.push(event);
    if (event.type === "text_delta") { firstDeltaAt ??= Date.now(); console.log(`delta: ${event.text}`); }
  }
});
child.stderr.on("data", data => stderr.push(data.toString()));
child.stdin.end("Reply with a short sentence without tools.");
const timeoutMs = process.platform === "win32" ? 180000 : 60000;
const timer = setTimeout(() => {
  stderr.push(`sidecar timed out after ${timeoutMs}ms`);
  if (process.platform === "win32") { try { execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true }); } catch {} }
  else child.kill("SIGKILL");
}, timeoutMs);
try {
  const code = await new Promise((resolve, reject) => { child.on("close", resolve); child.on("error", reject); });
  const note = stderr.join("").slice(-4000);
  assert.equal(code, 0, note || `OpenCode exited with ${code}`);
  assert.ok(events.some(event => event.type === "bridge_ready"), `missing bridge_ready. ${note}`);
  assert.equal(events.filter(event => event.type === "text_delta").map(event => event.text).join(""), "First token. Last token.");
  assert.ok(firstDeltaAt < providerCompletedAt - 500, "Text must arrive before API response completion");
  console.log(`PASS: actual OpenCode sidecar first delta arrived ${providerCompletedAt - firstDeltaAt} ms before completion.`);
} finally {
  clearTimeout(timer); server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}
