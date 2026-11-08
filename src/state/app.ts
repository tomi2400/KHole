import { create } from "zustand";
import { api } from "@/lib/api";
import { closeSession, openSession } from "@/state/session";

/** Start → Setup → Triage. The open session itself lives in `state/session`. */
export type Screen = "start" | "setup" | "triage";

interface AppState {
  screen: Screen;
  /** Set while a folder is being opened; the Start screen shows it. */
  opening: boolean;
  error: string | null;
}

export const useApp = create<AppState>(() => ({
  screen: "start",
  opening: false,
  error: null,
}));

/**
 * Opens a folder's session. An imported or dropped folder shows Setup first; a recent
 * session already has its settings, so it goes straight to Triage.
 */
export async function openFolder(path: string, screen: Screen = "setup"): Promise<void> {
  useApp.setState({ opening: true, error: null });
  try {
    const session = await api.openSession(path);
    openSession(session);
    useApp.setState({ screen });
  } catch (e) {
    useApp.setState({ error: String(e) });
  } finally {
    useApp.setState({ opening: false });
  }
}

export function goTo(screen: Exclude<Screen, "start">): void {
  useApp.setState({ screen });
}

export function goHome(): void {
  closeSession();
  useApp.setState({ screen: "start", error: null });
}

/** Native picker, then open. Cancelling does nothing. */
export async function importFolder(): Promise<void> {
  const path = await api.pickFolder();
  if (path) await openFolder(path);
}
