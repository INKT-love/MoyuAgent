import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(relative) {
  return JSON.parse(readFileSync(join(root, relative), "utf8"));
}

function cargoPackageVersion() {
  const lines = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").split(
    /\r?\n/,
  );
  let inPackage = false;
  for (const line of lines) {
    if (line.startsWith("[")) inPackage = line.trim() === "[package]";
    if (!inPackage) continue;
    const match = line.match(/^version\s*=\s*"([^"]+)"/);
    if (match) return match[1];
  }
  throw new Error("src-tauri/Cargo.toml is missing [package].version");
}

export function releaseVersions() {
  return {
    package: readJson("package.json").version,
    tauri: readJson("src-tauri/tauri.conf.json").version,
    cargo: cargoPackageVersion(),
  };
}

export function requireReleaseVersion(tag) {
  const versions = releaseVersions();
  const unique = [...new Set(Object.values(versions))];
  if (unique.length !== 1 || !unique[0]) {
    throw new Error(`Installer versions must match: ${JSON.stringify(versions)}`);
  }
  const version = unique[0];
  if (tag) {
    const expected = String(tag).trim().replace(/^v/i, "");
    if (expected !== version) {
      throw new Error(
        `Tag ${tag} does not match the installer version ${version}. Bump the version files before tagging.`,
      );
    }
  }
  return version;
}

const invoked = process.argv[1]?.replaceAll("\\", "/").endsWith("/check-release-version.mjs");
if (invoked) {
  console.log(requireReleaseVersion(process.argv[2]));
}
