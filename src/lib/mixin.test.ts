// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  MixinRuntime,
  frameSrc,
  isDocumentHtml,
  isPluginBlockedCommand,
  withBridge,
} from "./mixin";

describe("mixin runtime", () => {
  it("treats only .settings-page as a settings tab", () => {
    const runtime = new MixinRuntime();
    runtime.register({
      pluginId: "lab",
      pluginName: "测试页",
      select: ".settings-page",
      at: "RETURN",
      html: "<!doctype html><p>lab</p>",
    });
    runtime.register({
      pluginId: "tabs",
      pluginName: "标签",
      select: ".settings-tabs",
      at: "RETURN",
      html: "<p>tabs</p>",
    });
    expect(runtime.pages()).toHaveLength(1);
    expect(runtime.pages()[0]?.id).toBe("lab");
    expect(runtime.overlays()).toHaveLength(0);
  });

  it("injects the Moyu bridge into document html", () => {
    expect(isDocumentHtml("<!doctype html><p>x</p>")).toBe(true);
    const html = withBridge("<!doctype html><head></head><p>x</p>");
    expect(html).toContain("moyu-mixin-result");
    expect(withBridge(html)).toBe(html);
  });

  it("reuses blob frame urls for the same page html", () => {
    const first = frameSrc("lab", "<!doctype html><p>one</p>");
    const second = frameSrc("lab", "<!doctype html><p>one</p>");
    expect(first.startsWith("blob:")).toBe(true);
    expect(second).toBe(first);
    const third = frameSrc("lab", "<!doctype html><p>two</p>");
    expect(third.startsWith("blob:")).toBe(true);
    expect(third).not.toBe(first);
  });

  it("keeps store and skill management out of plugin reach", () => {
    for (const name of ["store_prepare", "store_install", "set_skill_enabled", "uninstall_skill"]) {
      expect(isPluginBlockedCommand(name)).toBe(true);
    }
    expect(isPluginBlockedCommand("get_plugin_settings")).toBe(false);
  });
});
