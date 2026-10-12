import { readFileSync } from "node:fs";

function extraInstructions() {
  const path = process.env.MOYU_HARNESS_INSTRUCTIONS_FILE;
  if (!path) return "";
  try {
    return readFileSync(path, "utf8").trim();
  } catch {
    return "";
  }
}

function appendPrompt(current, extra) {
  if (!extra) return current;
  const text = typeof current === "string" ? current : "";
  if (!text) return extra;
  if (text.includes(extra)) return text;
  return `${text}\n\n${extra}`;
}

export default async function MoyuHarnessLoader() {
  return {
    config(cfg) {
      const extra = extraInstructions();
      if (!extra) return;
      cfg.agent = cfg.agent || {};
      const build = cfg.agent.build || {};
      cfg.agent.build = {
        ...build,
        prompt: appendPrompt(build.prompt, extra),
      };
    },
    "experimental.chat.system.transform"(_input, output) {
      const extra = extraInstructions();
      if (!extra) return;
      const target = output && typeof output === "object" ? output : _input;
      if (!target || typeof target !== "object") return;
      if (Array.isArray(target.system)) {
        if (!target.system.some((item) => typeof item === "string" && item.includes(extra))) {
          target.system = [...target.system, extra];
        }
        return;
      }
      if (typeof target.system === "string" || target.system == null) {
        target.system = appendPrompt(target.system, extra);
      }
    },
  };
}
