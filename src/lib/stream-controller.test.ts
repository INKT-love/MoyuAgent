// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStreamController, type StreamEvent } from "./stream-controller";

function setup() {
  const callbacks = {
    requestId: "task-1",
    append: vi.fn(),
    status: vi.fn(),
    session: vi.fn(),
    terminal: vi.fn(),
    acknowledge: vi.fn(async (_sequence: number) => undefined),
    cancel: vi.fn(async () => undefined),
  };
  const stream = createStreamController(callbacks);
  const chunk = (sequence: number, text: string): StreamEvent => ({
    requestId: "task-1",
    sequence,
    kind: "chunk",
    text,
  });
  return { callbacks, stream, chunk };
}

describe("stream lifecycle", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("renders the first chunk immediately and batches subsequent chunks within 16ms", () => {
    const { callbacks, stream, chunk } = setup();
    stream.receive(chunk(0, "First"));
    expect(callbacks.append).toHaveBeenCalledWith("First");
    stream.receive(chunk(1, " second"));
    stream.receive(chunk(2, " third"));
    vi.advanceTimersByTime(15);
    expect(callbacks.append).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(callbacks.append).toHaveBeenLastCalledWith(" second third");
    expect(callbacks.acknowledge).not.toHaveBeenCalled();
    stream.unlisten();
  });

  it("acknowledges highest delivered sequence at 100ms independently of rendering", async () => {
    const { callbacks, stream, chunk } = setup();
    stream.receive(chunk(0, "a"));
    for (let sequence = 1; sequence <= 20; sequence++)
      stream.receive(chunk(sequence, "b"));
    await vi.advanceTimersByTimeAsync(99);
    expect(callbacks.acknowledge).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(callbacks.acknowledge).toHaveBeenCalledExactlyOnceWith(20);
    stream.unlisten();
  });

  it("ignores other tasks and stale sequences", () => {
    const { callbacks, stream, chunk } = setup();
    stream.receive({ ...chunk(0, "wrong"), requestId: "other" });
    stream.receive(chunk(2, "right"));
    stream.receive(chunk(2, "duplicate"));
    stream.receive(chunk(1, "stale"));
    stream.receive({ requestId: "other", sequence: 99, kind: "completed" });
    vi.advanceTimersByTime(16);
    expect(callbacks.append).toHaveBeenCalledExactlyOnceWith("right");
    expect(callbacks.terminal).not.toHaveBeenCalled();
    stream.unlisten();
  });

  it.each(["completed", "failed", "cancelled"] as const)(
    "flushes buffered text and closes exactly once on %s",
    (kind) => {
      const { callbacks, stream, chunk } = setup();
      stream.receive(chunk(0, "first"));
      stream.receive(chunk(1, " buffered"));
      const terminal = {
        requestId: "task-1",
        sequence: 2,
        kind,
        code: "test",
        message: "failure",
        retryable: true,
      } as StreamEvent;
      stream.receive(terminal);
      stream.receive({ ...terminal, sequence: 3 });
      stream.receive(chunk(4, "late"));
      vi.advanceTimersByTime(60_000);
      expect(callbacks.append).toHaveBeenCalledTimes(2);
      expect(callbacks.append).toHaveBeenLastCalledWith(" buffered");
      expect(callbacks.terminal).toHaveBeenCalledExactlyOnceWith(terminal);
      expect(callbacks.cancel).not.toHaveBeenCalled();
      stream.unlisten();
    },
  );

  it("resets deadman on heartbeats and cancels a silent backend", () => {
    const { callbacks, stream } = setup();
    vi.advanceTimersByTime(179_000);
    stream.receive({ requestId: "task-1", sequence: 0, kind: "heartbeat" });
    vi.advanceTimersByTime(179_999);
    expect(callbacks.terminal).not.toHaveBeenCalled();
    expect(callbacks.status).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(callbacks.terminal).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "failed", code: "frontend_timeout" }),
    );
    expect(callbacks.cancel).toHaveBeenCalledOnce();
    stream.unlisten();
  });

  it("disposes timers and cancels active work only once on unlisten", () => {
    const { callbacks, stream, chunk } = setup();
    stream.receive(chunk(0, "a"));
    stream.receive(chunk(1, "pending"));
    stream.unlisten();
    stream.unlisten();
    stream.receive(chunk(2, "late"));
    vi.advanceTimersByTime(60_000);
    expect(callbacks.append).toHaveBeenCalledExactlyOnceWith("a");
    expect(callbacks.terminal).not.toHaveBeenCalled();
    expect(callbacks.acknowledge).not.toHaveBeenCalled();
    expect(callbacks.cancel).toHaveBeenCalledOnce();
  });

  it("bounds buffered text and terminates safely on overflow", () => {
    const { callbacks, stream, chunk } = setup();
    stream.receive(chunk(0, "x".repeat(256 * 1024 + 1)));
    expect(callbacks.append).not.toHaveBeenCalled();
    expect(callbacks.terminal).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "failed", code: "buffer_limit" }),
    );
    expect(callbacks.cancel).toHaveBeenCalledOnce();
    stream.unlisten();
  });
});
