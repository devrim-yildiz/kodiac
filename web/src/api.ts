import type { Page } from "./types";

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}
export async function api<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const response = await fetch(path, {
    ...options,
    credentials: "same-origin",
    headers: {
      ...(options.body != null ? { "Content-Type": "application/json" } : {}),
      ...options.headers,
    },
  });
  if (!response.ok) {
    if (response.status === 401 && !path.endsWith("/login"))
      window.dispatchEvent(new Event("session-expired"));
    const body = await response.json().catch(() => ({}));
    throw new ApiError(
      body.error || `Request failed (${response.status})`,
      response.status,
    );
  }
  return response.status === 204 ? (undefined as T) : response.json();
}
// Render the first page immediately; fetch the remaining pages in bounded waves.
export async function allPages<T>(
  path: string,
  signal?: AbortSignal,
  update?: (items: T[], total: number) => void,
): Promise<T[]> {
  const separator = path.includes("?") ? "&" : "?";
  const page = (offset: number) =>
    api<Page<T>>(`${path}${separator}limit=200&offset=${offset}`, { signal });
  const first = await page(0);
  let items = first.items;
  update?.(items, first.total);
  for (let offset = 200; offset < first.total; offset += 600) {
    const offsets = [offset, offset + 200, offset + 400].filter(
      (n) => n < first.total,
    );
    const results = await Promise.all(offsets.map(page));
    items = [...items, ...results.flatMap((result) => result.items)];
    update?.(items, first.total);
  }
  // Catalog changes during pagination cannot create duplicate React keys.
  return [
    ...new Map(
      items.map((item) => [
        (item as { id?: string; media_item_id?: string }).id ??
          (item as { media_item_id?: string }).media_item_id,
        item,
      ]),
    ).values(),
  ];
}
export const message = (error: unknown) =>
  error instanceof Error
    ? error.message
    : "Something went wrong. Please try again.";
export const aborted = (error: unknown) =>
  error instanceof DOMException && error.name === "AbortError";
