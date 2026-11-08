/** Display helpers. Numbers go in `.tabular` cells so they do not jump. */

export function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

export function stem(path: string): string {
  const name = fileName(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

export function parentPath(path: string): string {
  const i = path.lastIndexOf("/");
  return i > 0 ? path.slice(0, i) : "/";
}

/** `/Users/me/Music/x` → `~/Music/x` */
export function tildify(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

/** 124 → "124", 123.97 → "123.97", 123.5 → "123.5" */
export function formatBpm(bpm: number | null): string {
  if (bpm == null) return "";
  return String(Math.round(bpm * 100) / 100);
}

/** Seconds → "m:ss" (or "h:mm:ss"). */
export function formatDuration(seconds: number | null): string {
  if (seconds == null) return "";
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}
