import { execFileSync } from "node:child_process";
import { mkdir, copyFile, chmod, access, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const version = "1.18.31";
const targets = {
  "win32-x64": ["opencode-windows-x64", "x86_64-pc-windows-msvc", "opencode.exe"],
  "darwin-arm64": ["opencode-darwin-arm64", "aarch64-apple-darwin", "opencode"],
  "darwin-x64": ["opencode-darwin-x64", "x86_64-apple-darwin", "opencode"],
  "linux-x64": ["opencode-linux-x64", "x86_64-unknown-linux-gnu", "opencode"],
  "linux-arm64": ["opencode-linux-arm64", "aarch64-unknown-linux-gnu", "opencode"],
};
const target = targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`Unsupported sidecar target: ${process.platform}-${process.arch}`);
const [packageName, triple, binary] = target;
const temp = await mkdtemp(path.join(tmpdir(), "moyu-sidecar-"));
const npmCli = process.env.npm_execpath;
if (!npmCli) throw new Error("Run this script using npm run sidecar:prepare");
// npm validates the registry tarball integrity. Lifecycle scripts are unnecessary for this binary package.
execFileSync(process.execPath, [npmCli, "install", "--prefix", temp, "--no-audit", "--no-fund", "--ignore-scripts", `${packageName}@${version}`], { stdio: "inherit" });
const source = path.join(temp, "node_modules", packageName, "bin", binary);
await access(source);
const destination = path.join(root, "src-tauri", "binaries", `opencode-${triple}${process.platform === "win32" ? ".exe" : ""}`);
await mkdir(path.dirname(destination), { recursive: true });
await copyFile(source, destination);
if (process.platform !== "win32") await chmod(destination, 0o755);
const actualVersion = execFileSync(destination, ["--version"], { encoding: "utf8", timeout: 30000 }).trim();
if (actualVersion !== version) throw new Error(`Sidecar version mismatch: ${actualVersion}`);
console.log(`Prepared OpenCode ${actualVersion}: ${destination}`);
