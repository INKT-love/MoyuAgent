// @vitest-environment node
import { describe, expect, it } from "vitest";
import { applyStreamTerminal } from "./stream-terminal";
import type { TerminalEvent } from "./stream-controller";

const failed = (code: string, message: string): TerminalEvent => ({
  requestId: "task-1",
  sequence: 2,
  kind: "failed",
  code,
  message,
  retryable: true,
});

describe("applyStreamTerminal", () => {
  it("keeps a successful reply when a later failure arrives", () => {
    expect(
      applyStreamTerminal(
        { text: "我是 grok-4.6", state: "streaming" },
        failed("opencode_error", "OpenCode reported an error"),
      ),
    ).toEqual({
      text: "我是 grok-4.6",
      state: "completed",
      error: undefined,
    });
  });

  it("keeps a real failure when the assistant never produced text", () => {
    expect(
      applyStreamTerminal(
        { text: "  ", state: "streaming" },
        failed("opencode_error", "OpenCode reported an error"),
      ),
    ).toEqual({
      text: "  ",
      state: "failed",
      error: "OpenCode reported an error",
    });
  });

  it("records completed and cancelled terminals as-is", () => {
    expect(
      applyStreamTerminal(
        { text: "hello", state: "streaming" },
        { requestId: "task-1", sequence: 2, kind: "completed" },
      ),
    ).toEqual({
      text: "hello",
      state: "completed",
      error: undefined,
    });
    expect(
      applyStreamTerminal(
        { text: "hello", state: "streaming" },
        { requestId: "task-1", sequence: 2, kind: "cancelled" },
      ),
    ).toEqual({
      text: "hello",
      state: "cancelled",
      error: undefined,
    });
  });
});
