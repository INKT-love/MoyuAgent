export type SessionStream = {
  conversationId: string;
  requestId: string;
  unlisten: (cancel?: boolean) => void;
  fail: (message: string) => void;
};

// Per-conversation stream map. Isolated so a later hot-update host can
// replace this module without a single global streaming flag.
export function createSessionStreams() {
  const items = new Map<string, SessionStream>();
  return {
    has(id: string) {
      return items.has(id);
    },
    get(id: string) {
      return items.get(id);
    },
    ids() {
      return [...items.keys()];
    },
    attach(stream: SessionStream) {
      const previous = items.get(stream.conversationId);
      items.set(stream.conversationId, stream);
      previous?.unlisten(true);
    },
    release(id: string) {
      items.delete(id);
    },
    disposeAll(cancel = true) {
      for (const stream of items.values()) stream.unlisten(cancel);
      items.clear();
    },
  };
}
