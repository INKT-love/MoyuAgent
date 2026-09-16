import { writeSync } from "node:fs";

export default async function MoyuStreamBridge() {
  let activeSession;
  const textParts = new Set();
  const emit = (event) => writeSync(1, JSON.stringify(event) + "\n");
  emit({ type: "bridge_ready", version: 1 });
  return {
    "chat.message": async (input) => {
      activeSession = input.sessionID;
    },
    event: async ({ event }) => {
      const properties = event.properties;
      if (event.type === "message.part.updated") {
        const part = properties.part;
        if (part.type === "text" && part.sessionID === activeSession) textParts.add(part.id);
      }
      if (event.type === "message.part.delta" && properties.sessionID === activeSession &&
          properties.field === "text" && textParts.has(properties.partID)) {
        // A synchronous pipe write bounds pending output even though OpenCode does not await event hooks.
        emit({ type: "text_delta", sessionID: properties.sessionID, partID: properties.partID, text: properties.delta });
      }
    },
  };
}
