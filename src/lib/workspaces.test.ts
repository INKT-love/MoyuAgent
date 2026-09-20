// @vitest-environment node
import { describe, expect, it } from "vitest";
import { workspaceKey, workspaceLabel } from "./workspaces";

describe("workspaceLabel", () => {
  it("returns a stored label when the path matches", () => {
    expect(
      workspaceLabel("D:\\workspace", { "D:/workspace": "Home" }),
    ).toBe("Home");
  });

  it("falls back to the folder name", () => {
    expect(workspaceLabel("D:\\workspace")).toBe("workspace");
  });

  it("treats Windows drive letters as case-insensitive", () => {
    expect(workspaceKey("D:\\Alpha")).toBe(workspaceKey("d:/alpha"));
  });
});
