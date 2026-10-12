import { readFileSync } from "node:fs";
import { join } from "node:path";

const STUB = "[earlier turn omitted; recall if needed]";
const KEEP_EXCHANGES = 2;

function loadState() {
  const directory = process.env.MOYU_PLUGIN_STATE_DIR;
  if (!directory) return {};
  try {
    return JSON.parse(readFileSync(join(directory, "spm.json"), "utf8"));
  } catch {
    return {};
  }
}

function roleOf(message) {
  return message?.role || message?.info?.role || "";
}

function stubText(message) {
  if (!message || typeof message !== "object") return;
  if (Array.isArray(message.parts)) {
    message.parts = message.parts.map((part) => {
      if (part?.type === "text" && typeof part.text === "string") {
        return { ...part, text: STUB };
      }
      if (part?.type === "reasoning" && typeof part.text === "string") {
        return { ...part, text: STUB };
      }
      return part;
    });
  }
  if (typeof message.content === "string") {
    message.content = STUB;
  } else if (Array.isArray(message.content)) {
    message.content = message.content.map((part) => {
      if (typeof part === "string") return STUB;
      if (part?.type === "text" && typeof part.text === "string") {
        return { ...part, text: STUB };
      }
      return part;
    });
  }
}

export function stubOlderTurns(messages, keep = KEEP_EXCHANGES) {
  if (!Array.isArray(messages) || keep < 1) return messages;
  const userIdx = [];
  for (let index = 0; index < messages.length; index += 1) {
    if (roleOf(messages[index]) === "user") userIdx.push(index);
  }
  const startKeep =
    userIdx.length <= keep ? 0 : userIdx[userIdx.length - keep];
  for (let index = 0; index < startKeep; index += 1) {
    stubText(messages[index]);
  }
  return messages;
}

export default async function SpmPlugin() {
  return {
    "experimental.chat.messages.transform"(_input, output) {
      const state = loadState();
      if (!state?.proxyEnabled) return;
      const target = output && Array.isArray(output.messages) ? output : _input;
      if (target && Array.isArray(target.messages)) {
        stubOlderTurns(target.messages, KEEP_EXCHANGES);
      }
    },
  };
}
