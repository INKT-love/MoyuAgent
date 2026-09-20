// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  compactModelName,
  modelChipLabel,
  parseReasoningEffort,
  reasoningEffortLabel,
} from "./models";

describe("compactModelName", () => {
  it("strips common display prefixes", () => {
    expect(compactModelName("GPT 5.6 Terra")).toBe("5.6 Terra");
    expect(compactModelName("Claude Sonnet 4.6")).toBe("Sonnet 4.6");
    expect(compactModelName("gpt-5.6-sol")).toBe("5.6-sol");
  });

  it("keeps grok ids that are not a Grok prefix", () => {
    expect(compactModelName("grok-4.6")).toBe("grok-4.6");
  });
});

describe("modelChipLabel", () => {
  it("shows the compact name and Chinese effort", () => {
    expect(modelChipLabel("gpt-5.6-terra", "GPT 5.6 Terra", "xhigh")).toBe(
      "5.6 Terra 极高",
    );
    expect(modelChipLabel("grok-4.6", undefined, "high")).toBe("grok-4.6 高");
  });

  it("falls back when no model is configured", () => {
    expect(modelChipLabel()).toBe("尚未配置模型");
  });
});

describe("parseReasoningEffort", () => {
  it("accepts known values and defaults the rest to high", () => {
    expect(parseReasoningEffort("XHIGH")).toBe("xhigh");
    expect(parseReasoningEffort("low")).toBe("low");
    expect(parseReasoningEffort("max")).toBe("high");
    expect(parseReasoningEffort()).toBe("high");
  });

  it("maps ids to Chinese labels", () => {
    expect(reasoningEffortLabel("xhigh")).toBe("极高");
    expect(reasoningEffortLabel("medium")).toBe("中");
    expect(reasoningEffortLabel("")).toBe("高");
  });
});
