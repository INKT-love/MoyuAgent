import { invoke, isTauri } from "@tauri-apps/api/core";

export interface Endpoint {
  index: number;
  name: string;
  baseUrl: string;
}
export interface Group {
  id: number;
  name: string;
  platform?: string;
}
export interface GroupModel {
  id: string;
  name: string;
}
export interface PluginInfo {
  id: string;
  name: string;
  description: string;
  version: string;
  enabled: boolean;
  bundled: boolean;
  hasModule: boolean;
  hasInstructions: boolean;
  hasUi: boolean;
}
export interface PluginUi {
  id: string;
  name: string;
  html: string;
}
export interface ConfigSummary {
  groupId: number;
  groupName: string;
  model: string;
  workspace: string;
  configPath: string;
}
export interface AppState {
  endpoint: Endpoint;
  endpoints: Endpoint[];
  authenticated: boolean;
  user: { email: string } | null;
  config: ConfigSummary | null;
  recentWorkspaces?: string[];
}
export interface PublicSettings {
  loginAgreementRequired: boolean;
  loginAgreementUrl: string | null;
  registrationUrl: string | null;
  agreementDocuments?: { title: string; content: string }[];
}
export interface LoginResult {
  user: { email: string } | null;
  requiresTwoFactor: boolean;
  tempToken: string | null;
}

export const ENDPOINTS: Endpoint[] = [
  { index: 0, name: "主线路", baseUrl: "https://inktandwkx.top" },
  { index: 1, name: "备用线路(CF)", baseUrl: "https://inkaicf.flymiku.top" },
];

export const desktop = isTauri();
export const initialState: AppState = {
  endpoint: ENDPOINTS[0],
  endpoints: ENDPOINTS,
  authenticated: false,
  user: null,
  config: null,
};

export function command<T>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    return Promise.reject(
      new Error("请在 Moyu Agent 桌面客户端中执行此操作。"),
    );
  return invoke<T>(name, args);
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error)
    return String(error.message);
  return "操作失败，请稍后重试。";
}

export function defaultModel(group?: Group): string {
  switch (group?.platform?.toLowerCase()) {
    case "openai":
      return "gpt-5.2";
    default:
      return "claude-sonnet-4-6";
  }
}

export function folderName(path?: string): string {
  return path?.split(/[\\/]/).filter(Boolean).at(-1) || "";
}

export function pickModel(models: GroupModel[], preferred?: string): string {
  if (preferred && models.some((model) => model.id === preferred))
    return preferred;
  return models[0]?.id ?? preferred ?? "";
}

export function supportedGroups(groups: Group[]): Group[] {
  return groups.filter(
    (group) =>
      !group.platform ||
      ["anthropic", "claude", "openai"].includes(group.platform.toLowerCase()),
  );
}
