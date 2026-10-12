// @vitest-environment node
import { describe, expect, it } from "vitest";
import { stubOlderTurns } from "../../src-tauri/resources/harness/spm/index.mjs";

describe("SPM phase-two window", () => {
  it("stubs text in older user turns and keeps the last two exchanges", () => {
    const messages = [
      { role: "user", parts: [{ type: "text", text: "old 1" }] },
      { role: "assistant", parts: [{ type: "text", text: "reply 1" }, { type: "tool", id: "t1" }] },
      { role: "user", parts: [{ type: "text", text: "old 2" }] },
      { role: "assistant", parts: [{ type: "text", text: "reply 2" }] },
      { role: "user", parts: [{ type: "text", text: "keep 1" }] },
      { role: "assistant", parts: [{ type: "text", text: "keep reply 1" }] },
      { role: "user", parts: [{ type: "text", text: "keep 2" }] },
      { role: "assistant", parts: [{ type: "text", text: "keep reply 2" }] },
    ];
    stubOlderTurns(messages, 2);
    expect(messages[0].parts[0].text).toBe("[earlier turn omitted; recall if needed]");
    expect(messages[1].parts[0].text).toBe("[earlier turn omitted; recall if needed]");
    expect(messages[1].parts[1]).toEqual({ type: "tool", id: "t1" });
    expect(messages[4].parts[0].text).toBe("keep 1");
    expect(messages[7].parts[0].text).toBe("keep reply 2");
  });
});
