export const REASONING_EFFORTS = [
  { id: "low", label: "低" },
  { id: "medium", label: "中" },
  { id: "high", label: "高" },
  { id: "xhigh", label: "极高" },
] as const;

export type ReasoningEffort = (typeof REASONING_EFFORTS)[number]["id"];

export const DEFAULT_REASONING_EFFORT: ReasoningEffort = "high";
export const REASONING_EFFORT_MAX = REASONING_EFFORTS.length - 1;

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

export function reasoningEffortIndex(value?: string | null): number {
  const effort = parseReasoningEffort(value);
  return Math.max(
    0,
    REASONING_EFFORTS.findIndex((item) => item.id === effort),
  );
}

export function reasoningEffortAt(index: number): ReasoningEffort {
  const clamped = Math.max(0, Math.min(REASONING_EFFORT_MAX, Math.round(index)));
  return REASONING_EFFORTS[clamped].id;
}

export function compactModelName(name: string): string {
  const compact = name.replace(/^(GPT |Claude |Grok |gpt-|claude-)/i, "").trim();
  return compact || name;
}

export function modelChipLabel(model?: string, name?: string): string {
  if (!model) return "尚未配置模型";
  return compactModelName(name || model);
}
