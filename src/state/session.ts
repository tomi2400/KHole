import { create } from "zustand";
import {
  api,
  events,
  type Decision,
  type Destination,
  type EditField,
  type Outcome,
  type ScanSummary,
  type Session,
  type Track,
  type Undone,
} from "@/lib/api";

/**
 * The open session: its settings, tracks, destinations and scan state. Setup and Triage
 * both read it, so a scan started on Setup keeps filling the list after Start.
 */
interface SessionState {
  session: Session | null;
  tracks: Track[] | null;
  destinations: Destination[];
  /** Set while a scan runs. */
  progress: { done: number; total: number } | null;
  summary: ScanSummary | null;
  error: string | null;
}

export const useSession = create<SessionState>(() => ({
  session: null,
  tracks: null,
  destinations: [],
  progress: null,
  summary: null,
  error: null,
}));

const set = useSession.setState;
const get = useSession.getState;

function fail(e: unknown): void {
  set({ error: String(e) });
}

/** Makes `session` the open one and syncs it with its folder. */
export function openSession(session: Session): void {
  if (get().session?.id !== session.id) {
    set({ session, tracks: null, destinations: [], progress: null, summary: null, error: null });
  } else {
    set({ session });
  }
  void refresh();
  void refreshDestinations();
  void rescan();
  // Finder changes (moved, renamed, deleted, put back) refresh the list by themselves.
  api.watchSession(session.id).catch(fail);
}

export function closeSession(): void {
  api.unwatchSession().catch(() => {});
  set({ session: null, tracks: null, destinations: [], progress: null, summary: null, error: null });
}

export function clearError(): void {
  set({ error: null });
}

function sessionId(): number | null {
  return get().session?.id ?? null;
}

export async function refresh(): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  try {
    const tracks = await api.listTracks(id);
    if (sessionId() === id) set({ tracks });
  } catch (e) {
    fail(e);
  }
}

async function refreshDestinations(): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  try {
    const destinations = await api.listDestinations(id);
    if (sessionId() === id) set({ destinations });
  } catch (e) {
    fail(e);
  }
}

// ── Scan ──────────────────────────────────────────────────────────────────────────────

/** One scan per session at a time; a second request joins the running one. */
const inflight = new Map<number, Promise<ScanSummary>>();

/**
 * Rescans after a change in Finder. Quiet: no progress bar for a small folder update. A
 * change during a running scan may have been missed by it, so one more scan follows.
 */
let again = false;
async function rescanForChange(): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  if (inflight.has(id)) {
    again = true;
    return;
  }
  do {
    again = false;
    await rescan({ quiet: true });
  } while (again && sessionId() === id);
}

void events.onFolderChanged((p) => {
  if (p.sessionId === sessionId()) void rescanForChange();
});

export async function rescan({ quiet = false }: { quiet?: boolean } = {}): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  let p = inflight.get(id);
  if (!p) {
    p = api.scanSession(id).finally(() => inflight.delete(id));
    inflight.set(id, p);
  }
  if (!quiet) set({ progress: { done: 0, total: 0 }, error: null });
  try {
    const summary = await p;
    if (sessionId() === id) set({ summary });
  } catch (e) {
    if (sessionId() === id) fail(e);
  } finally {
    if (sessionId() === id && !quiet) set({ progress: null });
    await refresh();
  }
}

/** While scanning, the list is refreshed at most this often. */
const LIVE_REFRESH_MS = 1000;
let lastLiveRefresh = 0;

void events.onScanProgress((p) => {
  if (p.sessionId !== sessionId()) return;
  if (get().progress) set({ progress: { done: p.done, total: p.total } });
  const now = Date.now();
  if (now - lastLiveRefresh > LIVE_REFRESH_MS) {
    lastLiveRefresh = now;
    void refresh();
  }
});

// ── Settings ──────────────────────────────────────────────────────────────────────────

async function updateSettings(call: (id: number) => Promise<Session>): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  try {
    const session = await call(id);
    if (sessionId() === id) set({ session });
  } catch (e) {
    fail(e);
  }
}

export async function setIncludeSub(value: boolean): Promise<void> {
  await updateSettings((id) => api.setIncludeSubfolders(id, value));
  void rescan();
}

export const setStartAt = (value: number) => updateSettings((id) => api.setStartAt(id, value));
export const setMode = (mode: Session["mode"]) => updateSettings((id) => api.setMode(id, mode));
export const setAutoNext = (value: boolean) =>
  updateSettings((id) => api.setAutoNext(id, value));

// ── Destinations ──────────────────────────────────────────────────────────────────────

async function updateDestinations(call: (id: number) => Promise<Destination[]>): Promise<void> {
  const id = sessionId();
  if (id == null) return;
  try {
    const destinations = await call(id);
    if (sessionId() === id) set({ destinations, error: null });
  } catch (e) {
    fail(e);
  }
}

/** Native picker, then add. Cancelling does nothing. */
export async function pickDestination(): Promise<void> {
  const path = await api.pickFolder("Add Destination").catch(() => null);
  if (path) await updateDestinations((id) => api.addDestination(id, path));
}

export const removeDestination = (destinationId: number) =>
  updateDestinations((id) => api.removeDestination(id, destinationId));

export const setDestinationHotkey = (destinationId: number, hotkey: number | null) =>
  updateDestinations((id) => api.setDestinationHotkey(id, destinationId, hotkey));

// ── Decisions ─────────────────────────────────────────────────────────────────────────

function replaceTrack(track: Track): void {
  const tracks = get().tracks;
  if (tracks) set({ tracks: tracks.map((t) => (t.id === track.id ? track : t)) });
}

/** Applies a decision to a track; the list and the tile counters follow. */
export async function decide(trackId: number, decision: Decision): Promise<Outcome> {
  const id = sessionId();
  if (id == null) throw new Error("no session");
  const outcome = await api.decide(id, trackId, decision);
  if (sessionId() === id) {
    replaceTrack(outcome.track);
    set({ error: null });
    if (outcome.destinationId != null) void refreshDestinations();
  }
  return outcome;
}

/** Renames the file or sets a tag; the list follows. Throws with the reason if refused. */
export async function editTrack(trackId: number, field: EditField, value: string): Promise<Track> {
  const id = sessionId();
  if (id == null) throw new Error("no session");
  const track = await api.editTrack(id, trackId, field, value);
  if (sessionId() === id) replaceTrack(track);
  return track;
}

/** Reverts the newest action; null if there is nothing left to undo. */
export async function undo(): Promise<Undone | null> {
  const id = sessionId();
  if (id == null) return null;
  const undone = await api.undo(id);
  if (undone && sessionId() === id) {
    replaceTrack(undone.track);
    set({ error: null });
    if (undone.destinationId != null) void refreshDestinations();
  }
  return undone;
}
