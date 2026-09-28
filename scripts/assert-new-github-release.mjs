const tag = String(process.argv[2] || "").trim();
const repo = process.env.GITHUB_REPOSITORY;
const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
if (!tag) throw new Error("Pass a release tag such as v0.1.2");
if (!repo) throw new Error("GITHUB_REPOSITORY is required");
if (!token) throw new Error("GITHUB_TOKEN is required");

const response = await fetch(
  `https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`,
  {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "moyu-agent-release",
      "X-GitHub-Api-Version": "2022-11-28",
    },
  },
);

if (response.status === 404) {
  console.log(`No existing release for ${tag}`);
  process.exit(0);
}

if (!response.ok) {
  throw new Error(
    `GitHub API ${response.status}: ${(await response.text()).slice(0, 300)}`,
  );
}

throw new Error(
  `Release ${tag} already exists. Bump the version and push a new tag.`,
);
