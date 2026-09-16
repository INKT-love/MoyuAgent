export type StreamEvent = { requestId: string; sequence: number } & (
  | { kind: "chunk"; text: string; sessionId?: string; partId?: string }
  | { kind: "status"; message: string; sessionId?: string }
  | { kind: "heartbeat" }
  | { kind: "completed"; sessionId?: string }
  | { kind: "failed"; code: string; message: string; retryable: boolean }
  | { kind: "cancelled" }
);

export type TerminalEvent = Extract<
  StreamEvent,
  { kind: "completed" | "failed" | "cancelled" }
>;

interface StreamCallbacks {
  requestId: string;
  append: (text: string) => void;
  status: (message: string) => void;
  session: (id: string) => void;
  terminal: (event: TerminalEvent) => void;
  acknowledge: (sequence: number) => Promise<unknown>;
  cancel: () => Promise<unknown>;
}

// Rendering and acknowledgements have separate clocks so UI frames never trigger IPC.
export function createStreamController(callbacks: StreamCallbacks) {
  let disposed = false;
  let terminated = false;
  let lastSequence = -1;
  let acknowledgedSequence = -1;
  let receivedChunk = false;
  let pending = "";
  let renderTimer: ReturnType<typeof setTimeout> | undefined;
  let acknowledgementTimer: ReturnType<typeof setTimeout> | undefined;
  let deadmanTimer: ReturnType<typeof setTimeout> | undefined;
  let acknowledgementInFlight = false;

  const flush = () => {
    if (renderTimer !== undefined) clearTimeout(renderTimer);
    renderTimer = undefined;
    if (pending) {
      const text = pending;
      pending = "";
      callbacks.append(text);
    }
  };

  const clearTimers = () => {
    if (renderTimer !== undefined) clearTimeout(renderTimer);
    if (acknowledgementTimer !== undefined) clearTimeout(acknowledgementTimer);
    if (deadmanTimer !== undefined) clearTimeout(deadmanTimer);
    renderTimer = acknowledgementTimer = deadmanTimer = undefined;
  };

  const acknowledge = async () => {
    acknowledgementTimer = undefined;
    if (
      disposed ||
      acknowledgementInFlight ||
      lastSequence <= acknowledgedSequence
    )
      return;
    acknowledgementInFlight = true;
    const sequence = lastSequence;
    try {
      await callbacks.acknowledge(sequence);
      acknowledgedSequence = sequence;
    } catch {
      // A closing backend may already have removed a terminal stream.
    } finally {
      acknowledgementInFlight = false;
      if (
        !disposed &&
        !terminated &&
        lastSequence > acknowledgedSequence &&
        acknowledgementTimer === undefined
      ) {
        acknowledgementTimer = setTimeout(() => void acknowledge(), 100);
      }
    }
  };

  const finish = (event: TerminalEvent, cancel = false) => {
    if (disposed || terminated) return;
    terminated = true;
    flush();
    clearTimers();
    callbacks.terminal(event);
    void acknowledge();
    if (cancel) void callbacks.cancel().catch(() => undefined);
  };

  const armDeadman = () => {
    if (deadmanTimer !== undefined) clearTimeout(deadmanTimer);
    deadmanTimer = setTimeout(
      () =>
        finish(
          {
            requestId: callbacks.requestId,
            sequence: lastSequence,
            kind: "failed",
            code: "frontend_timeout",
            message: "连接已中断，任务已停止。请重新发送。",
            retryable: true,
          },
          true,
        ),
      30_000,
    );
  };

  armDeadman();

  return {
    receive(event: StreamEvent) {
      if (
        disposed ||
        terminated ||
        event.requestId !== callbacks.requestId ||
        event.sequence <= lastSequence
      )
        return;
      if (!Number.isSafeInteger(event.sequence) || event.sequence < 0) return;
      lastSequence = event.sequence;
      armDeadman();
      if ("sessionId" in event && event.sessionId)
        callbacks.session(event.sessionId);
      if (
        event.kind === "completed" ||
        event.kind === "failed" ||
        event.kind === "cancelled"
      ) {
        finish(event);
        return;
      }
      if (acknowledgementTimer === undefined && !acknowledgementInFlight)
        acknowledgementTimer = setTimeout(() => void acknowledge(), 100);
      if (event.kind === "chunk") {
        pending += event.text;
        if (pending.length > 256 * 1024) {
          pending = "";
          finish(
            {
              requestId: callbacks.requestId,
              sequence: lastSequence,
              kind: "failed",
              code: "buffer_limit",
              message: "输出速度超出处理上限，任务已停止。",
              retryable: true,
            },
            true,
          );
          return;
        }
        if (!receivedChunk) {
          receivedChunk = true;
          flush();
        } else if (renderTimer === undefined)
          renderTimer = setTimeout(flush, 16);
      } else if (event.kind === "status") callbacks.status(event.message);
    },
    fail(message: string) {
      finish(
        {
          requestId: callbacks.requestId,
          sequence: lastSequence,
          kind: "failed",
          code: "ipc_error",
          message,
          retryable: true,
        },
        true,
      );
    },
    unlisten(cancel = true) {
      if (disposed) return;
      disposed = true;
      clearTimers();
      pending = "";
      if (cancel && !terminated) void callbacks.cancel().catch(() => undefined);
    },
  };
}
