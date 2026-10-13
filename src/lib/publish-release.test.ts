// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  GitHubError,
  createClient,
  isTransient,
  publishRelease,
  withRetries,
} from "../../scripts/publish-github-release.mjs";

const noWait = { sleep: async () => {}, delays: [0] };
const file = { path: "C:/out/setup.exe", read: async () => new Uint8Array([1, 2, 3]) };

type Release = { id: number; draft: boolean; tag_name: string; upload_url: string; html_url: string; assets: { name: string; state: string; size: number }[] };

/** In-memory GitHub with scripted failures per method. */
function fakeGitHub(failures: Record<string, (number | undefined)[]> = {}) {
  let nextId = 1;
  const releases: Release[] = [];
  const calls: string[] = [];
  const maybeFail = (method: string) => {
    calls.push(method);
    const queue = failures[method];
    if (queue?.length) {
      const status = queue.shift();
      throw new GitHubError(status, `boom ${status}`);
    }
  };
  const client = {
    async releaseByTag(tag: string) {
      maybeFail("releaseByTag");
      return releases.find((r) => r.tag_name === tag && !r.draft) ?? null;
    },
    async draftByTag(tag: string) {
      maybeFail("draftByTag");
      return releases.find((r) => r.tag_name === tag && r.draft) ?? null;
    },
    async create(tag: string) {
      maybeFail("create");
      const release = { id: nextId++, draft: true, tag_name: tag, upload_url: "u{?name}", html_url: `https://x/${tag}`, assets: [] };
      releases.push(release);
      return release;
    },
    async upload(release: Release, name: string, data: Uint8Array) {
      maybeFail("upload");
      release.assets.push({ name, state: "uploaded", size: data.length });
    },
    async publish(release: Release) {
      maybeFail("publish");
      release.draft = false;
      return release;
    },
    async remove(release: Release) {
      maybeFail("remove");
      releases.splice(releases.indexOf(release), 1);
    },
  };
  return { client, releases, calls };
}

const run = (gh: ReturnType<typeof fakeGitHub>, extra = {}) =>
  publishRelease({ client: gh.client, tag: "v0.1.4", notes: "n", file, size: 3, retry: noWait, log: () => {}, ...extra });

describe("transient error detection", () => {
  it("retries network, 408, 429 and 5xx but not other 4xx", () => {
    for (const status of [undefined, 408, 429, 500, 502, 503]) expect(isTransient(new GitHubError(status, ""))).toBe(true);
    for (const status of [400, 401, 403, 404, 409, 422]) expect(isTransient(new GitHubError(status, ""))).toBe(false);
  });

  it("stops after the attempt limit", async () => {
    let calls = 0;
    await expect(
      withRetries(async () => { calls += 1; throw new GitHubError(500, "x"); }, { attempts: 3, ...noWait }),
    ).rejects.toThrow("x");
    expect(calls).toBe(3);
  });
});

describe("publishRelease", () => {
  it("creates a draft, uploads, then publishes", async () => {
    const gh = fakeGitHub();
    const release = await run(gh);
    expect(release.draft).toBe(false);
    expect(release.assets).toEqual([{ name: "setup.exe", state: "uploaded", size: 3 }]);
    expect(gh.calls).toEqual(["releaseByTag", "draftByTag", "create", "upload", "publish"]);
  });

  it("recovers from the HTTP 500 seen on v0.1.4", async () => {
    const gh = fakeGitHub({ create: [500] });
    await run(gh);
    expect(gh.releases).toHaveLength(1);
    expect(gh.releases[0].draft).toBe(false);
  });

  it("removes the leftover draft when the upload fails midway", async () => {
    const gh = fakeGitHub({ upload: [502] });
    await run(gh);
    expect(gh.calls).toContain("remove");
    expect(gh.releases).toHaveLength(1);
    expect(gh.releases[0].assets).toHaveLength(1);
  });

  it("treats a release finished by a timed-out earlier attempt as done", async () => {
    const gh = fakeGitHub();
    const original = gh.client.publish;
    let first = true;
    gh.client.publish = async (release: Release) => {
      const result = await original(release);
      if (first) { first = false; throw new GitHubError(504, "gateway timeout"); }
      return result;
    };
    const release = await run(gh);
    expect(release.draft).toBe(false);
    expect(gh.releases).toHaveLength(1);
  });

  it("refuses to replace an existing published release", async () => {
    const gh = fakeGitHub();
    await run(gh);
    await expect(run(gh)).rejects.toThrow(/already exists/);
    expect(gh.releases).toHaveLength(1);
  });

  it("does not retry permanent errors", async () => {
    const gh = fakeGitHub({ create: [403] });
    await expect(run(gh)).rejects.toThrow("boom 403");
    expect(gh.calls.filter((call) => call === "create")).toHaveLength(1);
  });
});

describe("GitHub client", () => {
  it("maps HTTP status and network failures to GitHubError", async () => {
    const statuses = [500, 404];
    const fetchImpl = async () => {
      const status = statuses.shift();
      if (status === undefined) throw new Error("socket hang up");
      return new Response("{}", { status });
    };
    const client = createClient({ repo: "o/r", token: "t", fetchImpl });
    await expect(client.releaseByTag("v1")).rejects.toMatchObject({ status: 500 });
    expect(await client.releaseByTag("v1")).toBeNull();
    await expect(client.releaseByTag("v1")).rejects.toMatchObject({ status: undefined });
  });

  it("builds the upload URL from the release template", async () => {
    const urls: string[] = [];
    const fetchImpl = async (url: string) => { urls.push(url); return new Response("{}", { status: 201 }); };
    const client = createClient({ repo: "o/r", token: "t", fetchImpl });
    await client.upload({ upload_url: "https://uploads.github.com/repos/o/r/releases/9/assets{?name,label}" }, "Moyu Agent.exe", new Uint8Array(1));
    expect(urls[0]).toBe("https://uploads.github.com/repos/o/r/releases/9/assets?name=Moyu%20Agent.exe");
  });
});
