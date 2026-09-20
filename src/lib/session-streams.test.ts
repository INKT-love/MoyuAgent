// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import { createSessionStreams } from "./session-streams";

describe("createSessionStreams", () => {
  it("tracks multiple conversations and only cancels the replaced one", () => {
    const streams = createSessionStreams();
    const first = {
      conversationId: "a",
      requestId: "r1",
      unlisten: vi.fn(),
      fail: vi.fn(),
    };
    const second = {
      conversationId: "b",
      requestId: "r2",
      unlisten: vi.fn(),
      fail: vi.fn(),
    };
    streams.attach(first);
    streams.attach(second);
    expect(streams.ids()).toEqual(["a", "b"]);
    expect(first.unlisten).not.toHaveBeenCalled();

    const replacement = {
      conversationId: "a",
      requestId: "r3",
      unlisten: vi.fn(),
      fail: vi.fn(),
    };
    streams.attach(replacement);
    expect(first.unlisten).toHaveBeenCalledWith(true);
    expect(streams.get("a")?.requestId).toBe("r3");
    expect(streams.has("b")).toBe(true);

    streams.release("b");
    expect(streams.ids()).toEqual(["a"]);
    streams.disposeAll(true);
    expect(replacement.unlisten).toHaveBeenCalledWith(true);
    expect(streams.ids()).toEqual([]);
  });
});
