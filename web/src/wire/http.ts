// Small REST helpers for the `bh-web` endpoints that are not daemon wire traffic.
export async function api<T = any>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers ?? {}) },
  });
  if (!response.ok) throw new Error((await response.text()) || response.statusText);
  return (await response.json()) as T;
}
