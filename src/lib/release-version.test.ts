// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  releaseVersions,
  requireReleaseVersion,
} from "../../scripts/check-release-version.mjs";

describe("release version", () => {
  it("keeps package, tauri and cargo versions in sync", () => {
    const versions = releaseVersions();
    expect(versions.package).toBe(versions.tauri);
    expect(versions.package).toBe(versions.cargo);
    expect(versions.package).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it("accepts a matching tag and rejects a mismatch", () => {
    const version = requireReleaseVersion();
    expect(requireReleaseVersion(`v${version}`)).toBe(version);
    expect(() => requireReleaseVersion("0.0.0")).toThrow(/does not match/);
  });
});
