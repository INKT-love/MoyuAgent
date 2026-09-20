export const PERMISSION_MODES = [
  {
    id: "ask",
    label: "请求批准",
    chip: "请求批准",
    description: "编辑外部文件和使用互联网时始终询问",
  },
  {
    id: "assist",
    label: "帮我批准",
    chip: "帮我批准",
    description: "仅对检测到的风险操作请求批准",
  },
  {
    id: "full",
    label: "完全访问权限",
    chip: "完全访问",
    description: "可不受限制地访问互联网和你电脑上的任何文件",
  },
] as const;

export type PermissionMode = (typeof PERMISSION_MODES)[number]["id"];

export const DEFAULT_PERMISSION_MODE: PermissionMode = "assist";

export function parsePermissionMode(value?: string | null): PermissionMode {
  switch (value?.trim().toLowerCase()) {
    case "ask":
      return "ask";
    case "full":
      return "full";
    case "assist":
      return "assist";
    default:
      return DEFAULT_PERMISSION_MODE;
  }
}

export function permissionChipLabel(value?: string | null): string {
  const mode = parsePermissionMode(value);
  return PERMISSION_MODES.find((item) => item.id === mode)?.chip ?? "帮我批准";
}
