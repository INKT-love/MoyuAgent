import { readFileSync } from "node:fs";

export default async function MoyuHarnessLoader() {
  return {
    config(cfg) {
      const path = process.env.MOYU_HARNESS_INSTRUCTIONS_FILE;
      if (!path) return;
      let extra = "";
      try {
        extra = readFileSync(path, "utf8").trim();
      } catch {
        return;
      }
      if (!extra) return;
      cfg.agent = cfg.agent || {};
      const build = cfg.agent.build || {};
      const current = typeof build.prompt === "string" ? build.prompt : "";
      cfg.agent.build = {
        ...build,
        prompt: [current, extra].filter(Boolean).join("\n\n"),
      };
    },
  };
}
