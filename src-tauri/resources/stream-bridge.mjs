import { writeSync } from "node:fs";

export default async function MoyuStreamBridge() {
  let activeSession;
  const textParts = new Set();
  const userMessages = new Set();
  const emit = (event) => writeSync(1, JSON.stringify(event) + "\n");
  emit({ type: "bridge_ready", version: 1 });
  const rememberRole = (info) => {
    if (!info?.id) return;
    if (info.role === "user" || info.role === "system") userMessages.add(info.id);
    if (info.role === "assistant") userMessages.delete(info.id);
  };
  return {
    "chat.message": async (input) => {
      activeSession = input.sessionID;
    },
    event: async ({ event }) => {
      const properties = event.properties;
      if (event.type === "message.updated" || event.type === "message.created") {
        rememberRole(properties.info || properties);
      }
      if (event.type === "message.part.updated") {
        const part = properties.part;
        if (part.type === "text" && part.ignored !== true && part.sessionID === activeSession && !userMessages.has(part.messageID)) {
          textParts.add(part.id);
        }
      }
      if (event.type === "message.part.delta" && properties.sessionID === activeSession &&
          properties.field === "text" && textParts.has(properties.partID) &&
          !userMessages.has(properties.messageID)) {
        // A synchronous pipe write bounds pending output even though OpenCode does not await event hooks.
        emit({ type: "text_delta", sessionID: properties.sessionID, partID: properties.partID, text: properties.delta });
      }
    },
  };
}
