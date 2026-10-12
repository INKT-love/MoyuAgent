// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { StoreItem } from "./api";
import { matchesQuery } from "./store";

const item: StoreItem = {
  id: "git-release",
  kind: "skill",
  name: "发布助手",
  description: "Create tagged releases",
  version: "1.0.0",
  author: "Moyu",
  tags: ["git", "release"],
  url: "https://store.example/a.zip",
  sha256: "0".repeat(64),
  installed: false,
  updateAvailable: false,
  enabled: false,
};

describe("store search", () => {
  it("matches name, id, description, author and tags case-insensitively", () => {
    for (const query of ["发布", "GIT-REL", "tagged", "moyu", "Release"]) {
      expect(matchesQuery(item, query)).toBe(true);
    }
  });

  it("treats an empty query as match-all and rejects unrelated text", () => {
    expect(matchesQuery(item, "   ")).toBe(true);
    expect(matchesQuery(item, "database")).toBe(false);
  });
});
