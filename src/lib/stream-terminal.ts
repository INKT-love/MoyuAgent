import type { TerminalEvent } from "./stream-controller";

export type StreamMessageState = {
  text: string;
  state?: "streaming" | "completed" | "failed" | "cancelled";
  error?: string;
};

export function applyStreamTerminal<T extends StreamMessageState>(
  message: T,
  terminal: TerminalEvent,
): T {
  if (terminal.kind === "failed" && message.text.trim()) {
    return { ...message, state: "completed", error: undefined };
  }
  return {
    ...message,
    state: terminal.kind,
    error: terminal.kind === "failed" ? terminal.message : undefined,
  };
}
