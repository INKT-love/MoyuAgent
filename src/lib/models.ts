export const REASONING_EFFORTS = [
  { id: "xhigh", label: "极高" },
  { id: "high", label: "高" },
  { id: "medium", label: "中" },
  { id: "low", label: "低" },
] as const;

export type ReasoningEffort = (typeof REASONING_EFFORTS)[number]["id"];

export const DEFAULT_REASONING_EFFORT: ReasoningEffort = "high";

export function parseReasoningEffort(
  value?: string | null,
): ReasoningEffort {
  switch (value?.trim().toLowerCase()) {
    case "low":
      return "low";
    case "medium":
      return "medium";
    case "high":
      return "high";
    case "xhigh":
      return "xhigh";
    default:
      return DEFAULT_REASONING_EFFORT;
  }
}

export function reasoningEffortLabel(value?: string | null): string {
  const effort = parseReasoningEffort(value);
  return REASONING_EFFORTS.find((item) => item.id === effort)?.label ?? "高";
}

export function compactModelName(name: string): string {
  const compact = name.replace(/^(GPT |Claude |Grok |gpt-|claude-)/i, "").trim();
  return compact || name;
}

export function modelChipLabel(
  model?: string,
  name?: string,
  effort?: string,
): string {
  if (!model) return "尚未配置模型";
  return `${compactModelName(name || model)} ${reasoningEffortLabel(effort)}`;
}
