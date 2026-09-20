export function workspaceKey(path: string): string {
  const normalized = path.replace(/^\\\\\?\\UNC\\/i, "//").replace(/^\\\\\?\\/, "").replace(/\\/g, "/");
  const key = normalized.replace(/\/+$/, "") || "/";
  return /^(?:[a-z]:|\/\/)/i.test(key) ? key.toLowerCase() : key;
}

export function workspaceLabel(
  path?: string,
  labels?: Record<string, string>,
): string {
  if (!path) return "";
  if (labels) {
    const key = workspaceKey(path);
    for (const [stored, label] of Object.entries(labels)) {
      if (workspaceKey(stored) === key && label.trim()) return label.trim();
    }
  }
  return path.split(/[\\/]/).filter(Boolean).at(-1) || path;
}
