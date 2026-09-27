// @vitest-environment node
import { describe, expect, it } from "vitest";
import {
  compactModelName,
  modelChipLabel,
  parseReasoningEffort,
  reasoningEffortAt,
  reasoningEffortIndex,
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
  it("shows only the compact model name", () => {
    expect(modelChipLabel("gpt-5.6-terra", "GPT 5.6 Terra")).toBe("5.6 Terra");
    expect(modelChipLabel("grok-4.6")).toBe("grok-4.6");
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

  it("maps slider positions from low to xhigh", () => {
    expect(reasoningEffortIndex("low")).toBe(0);
    expect(reasoningEffortIndex("high")).toBe(2);
    expect(reasoningEffortIndex("xhigh")).toBe(3);
    expect(reasoningEffortAt(0)).toBe("low");
    expect(reasoningEffortAt(3)).toBe("xhigh");
    expect(reasoningEffortAt(9)).toBe("xhigh");
  });
});
