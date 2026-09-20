// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  parsePermissionMode,
  permissionChipLabel,
} from "./permissions";

describe("parsePermissionMode", () => {
  it("accepts known values and defaults the rest to assist", () => {
    expect(parsePermissionMode("ASK")).toBe("ask");
    expect(parsePermissionMode("full")).toBe("full");
    expect(parsePermissionMode("yolo")).toBe("assist");
    expect(parsePermissionMode()).toBe("assist");
  });
});

describe("permissionChipLabel", () => {
  it("uses the short chip labels from Codex", () => {
    expect(permissionChipLabel("ask")).toBe("请求批准");
    expect(permissionChipLabel("assist")).toBe("帮我批准");
    expect(permissionChipLabel("full")).toBe("完全访问");
    expect(permissionChipLabel()).toBe("帮我批准");
  });
});
