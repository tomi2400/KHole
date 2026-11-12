/**
 * View preferences of this Mac (column order, hidden columns, the "hide done" filter).
 * Kept in the WebView's localStorage: they are conveniences, not session data, so losing
 * them only resets the view. Every access is guarded.
 */

export function loadPref<T>(key: string, fallback: T, valid: (v: unknown) => v is T): T {
  try {
    const raw = localStorage.getItem(`khole.${key}`);
    if (raw == null) return fallback;
    const value: unknown = JSON.parse(raw);
    return valid(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

export function savePref(key: string, value: unknown): void {
  try {
    localStorage.setItem(`khole.${key}`, JSON.stringify(value));
  } catch {
    // Storage blocked or full: the view simply is not remembered.
  }
}

export function removePref(key: string): void {
  try {
    localStorage.removeItem(`khole.${key}`);
  } catch {
    // ignore
  }
}

export const isStringArray = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((x) => typeof x === "string");

export const isBoolean = (v: unknown): v is boolean => typeof v === "boolean";
