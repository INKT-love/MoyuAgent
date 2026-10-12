import type { StoreItem } from "./api";

/** Case-insensitive match over everything a user might search a store entry by. */
export function matchesQuery(item: StoreItem, query: string) {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return [item.name, item.id, item.description, item.author, ...item.tags]
    .join("\n")
    .toLowerCase()
    .includes(needle);
}
