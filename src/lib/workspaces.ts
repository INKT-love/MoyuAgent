export function workspaceKey(path: string): string {
  const normalized = path.replace(/^\\\\\?\\UNC\\/i, "//").replace(/^\\\\\?\\/, "").replace(/\\/g, "/");
  const key = normalized.replace(/\/+$/, "") || "/";
  return /^(?:[a-z]:|\/\/)/i.test(key) ? key.toLowerCase() : key;
}
