// Creates a GitHub Release and uploads the installer, retrying transient GitHub failures
// (HTTP 5xx, 429, network errors). Permanent errors (4xx) fail immediately.
// Never replaces a complete release; a half-created one left by a failed attempt
// (no installer asset yet) is deleted before retrying.
import { readFile, stat } from "node:fs/promises";
import { basename } from "node:path";

export class GitHubError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

/** True for failures worth retrying: no status (network) or 408/429/5xx. */
export function isTransient(error) {
  const status = error?.status;
  if (status === undefined || status === null) return true;
  return status === 408 || status === 429 || status >= 500;
}

export async function withRetries(work, { attempts = 4, delays = [15000, 45000, 90000], sleep, log = () => {} } = {}) {
  const wait = sleep ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await work(attempt);
    } catch (error) {
      if (attempt >= attempts || !isTransient(error)) throw error;
      const delay = delays[Math.min(attempt - 1, delays.length - 1)];
      log(`Attempt ${attempt} failed (${error.status ?? "network"}: ${error.message}); retrying in ${delay / 1000}s`);
      await wait(delay);
    }
  }
}

export function createClient({ repo, token, fetchImpl = fetch }) {
  const headers = {
    Authorization: `Bearer ${token}`,
    Accept: "application/vnd.github+json",
    "User-Agent": "moyu-agent-release",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  const call = async (url, init = {}) => {
    let response;
    try {
      response = await fetchImpl(url, { ...init, headers: { ...headers, ...init.headers } });
    } catch (error) {
      throw new GitHubError(undefined, `network error: ${error.message}`);
    }
    if (response.status === 204) return null;
    const text = await response.text();
    if (!response.ok) throw new GitHubError(response.status, `HTTP ${response.status}: ${text.slice(0, 300)}`);
    return text ? JSON.parse(text) : null;
  };
  const api = `https://api.github.com/repos/${repo}`;
  return {
    async releaseByTag(tag) {
      try {
        return await call(`${api}/releases/tags/${encodeURIComponent(tag)}`);
      } catch (error) {
        if (error.status === 404) return null;
        throw error;
      }
    },
    // Drafts are invisible to the by-tag endpoint, so list them too.
    async draftByTag(tag) {
      const releases = await call(`${api}/releases?per_page=30`);
      return releases.find((release) => release.draft && release.tag_name === tag) ?? null;
    },
    create(tag, notes) {
      return call(`${api}/releases`, {
        method: "POST",
        body: JSON.stringify({ tag_name: tag, name: tag, body: notes, draft: true }),
      });
    },
    upload(release, name, data) {
      const url = release.upload_url.replace(/\{.*\}$/, "") + `?name=${encodeURIComponent(name)}`;
      return call(url, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream", "Content-Length": String(data.length) },
        body: data,
      });
    },
    publish(release) {
      return call(`${api}/releases/${release.id}`, { method: "PATCH", body: JSON.stringify({ draft: false }) });
    },
    remove(release) {
      return call(`${api}/releases/${release.id}`, { method: "DELETE" });
    },
  };
}

/**
 * The release is created as a draft, the installer uploaded, then published, so users never
 * see a release without its installer. A leftover draft for this tag is a failed earlier
 * attempt and is removed; a published release is never touched.
 */
export async function publishRelease({ client, tag, notes, file, assetName, size, retry = {}, log = console.log }) {
  const name = assetName ?? basename(file.path);
  return withRetries(
    async (attempt) => {
      const published = await client.releaseByTag(tag);
      if (published && !published.draft) {
        const complete = published.assets?.some((asset) => asset.name === name && asset.state === "uploaded" && asset.size === size);
        if (complete && attempt > 1) {
          log(`Release ${tag} was completed by an earlier attempt`);
          return published;
        }
        throw new GitHubError(409, `Release ${tag} already exists. Bump the version and push a new tag.`);
      }
      const leftover = await client.draftByTag(tag);
      if (leftover) {
        log(`Removing unfinished draft release ${leftover.id} from a failed attempt`);
        await client.remove(leftover);
      }
      const draft = await client.create(tag, notes);
      await client.upload(draft, name, await file.read());
      const release = await client.publish(draft);
      log(`Published ${tag}: ${release.html_url}`);
      return release;
    },
    { log, ...retry },
  );
}

const invoked = process.argv[1]?.replaceAll("\\", "/").endsWith("/publish-github-release.mjs");
if (invoked) {
  const [tag, path, assetName] = process.argv.slice(2);
  const repo = process.env.GITHUB_REPOSITORY;
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (!tag || !path) throw new Error("Usage: publish-github-release.mjs <tag> <installer> [asset-name]");
  if (!repo || !token) throw new Error("GITHUB_REPOSITORY and GITHUB_TOKEN are required");
  const version = tag.replace(/^v/, "");
  const sha = process.env.GITHUB_SHA ?? "local";
  const { size } = await stat(path);
  await publishRelease({
    client: createClient({ repo, token }),
    tag,
    notes: `Windows x64 NSIS installer for Moyu Agent ${version}.\nBuilt by GitHub Actions from ${sha}.`,
    file: { path, read: () => readFile(path) },
    assetName,
    size,
  });
}
