import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Typed wrappers around the Rust commands (src-tauri/src/commands.rs). */

export interface AppInfo {
  version: string;
  dbPath: string;
  schemaVersion: number;
}

export interface AnalyzerInfo {
  name: string;
  version: string;
  essentia: string;
}

export interface Session {
  id: number;
  sourcePath: string;
  includeSub: boolean;
  mode: "move" | "copy";
  startAt: number;
  autoNext: boolean;
  currentTrack: number | null;
}

export interface RecentSession {
  id: number;
  sourcePath: string;
  total: number;
  remaining: number;
  /** Unix milliseconds. */
  updatedAt: number;
  /** False if the folder is gone (moved, deleted, unmounted volume). */
  available: boolean;
}

export type TrackStatus = "pending" | "kept" | "moved" | "copied" | "trashed" | "missing";

export interface Track {
  id: number;
  path: string;
  title: string | null;
  artist: string | null;
  bpm: number | null;
  musicalKey: string | null;
  /** Seconds. */
  duration: number | null;
  format: string | null;
  /** kbps. */
  bitrate: number | null;
  status: TrackStatus;
}

export interface Destination {
  id: number;
  path: string;
  /** 1–9 or 0 (the 10th); null = no number key. */
  hotkey: number | null;
  position: number;
  /** Tracks sorted into it in this session. */
  count: number;
}

export type Decision = { kind: "keep" } | { kind: "trash" } | { kind: "sort"; destinationId: number };

export type ActionKind = "move" | "copy" | "trash" | "keep" | "rename" | "tag";

/** What can be edited in the list: two tags and the file name (without extension). */
export type EditField = "title" | "artist" | "file";

export interface Outcome {
  /** The track row after the action (new path and status). */
  track: Track;
  kind: ActionKind;
  destinationId: number | null;
  /** Set when the name was taken in the destination: the name the file got. */
  renamedTo: string | null;
  /**
   * The destination already had this very track (same name and content): nothing was
   * copied, and on a move the source went to the Trash. The path of the file already there.
   */
  alreadyThere: string | null;
}

export interface Undone {
  track: Track;
  kind: ActionKind;
  destinationId: number | null;
  /** Something did not go back exactly (name taken, file gone meanwhile, …). */
  note: string | null;
}

export interface ScanSummary {
  total: number;
  added: number;
  removed: number;
  /** Trashed or moved tracks whose file came back into the folder. */
  returned: number;
  /** Known tracks found under a new name or in another subfolder (renamed in Finder). */
  renamed: number;
  /** Decided tracks whose file disappeared from the folder. */
  missing: number;
  unreadable: number;
}

export interface ScanProgress {
  sessionId: number;
  done: number;
  total: number;
}

export interface FolderChanged {
  sessionId: number;
}

/** An app that can open a track (Finder's "Open With"). */
export interface OpenWithApp {
  name: string;
  path: string;
  isDefault: boolean;
}

export interface Peaks {
  perSec: number;
  /** Seconds, exact (from the decoded frame count). */
  duration: number;
  /** Loudness (RMS) per bucket, 0..1: what the waveform draws. */
  rms: Float32Array;
  /** Sample peak per bucket, 0..1. */
  peak: Float32Array;
}

/** Parses the binary reply of `track_peaks` (layout in src-tauri/src/audio/peaks.rs). */
function parsePeaks(buf: ArrayBuffer): Peaks {
  const view = new DataView(buf);
  const raw = new Int8Array(buf, 16);
  const n = raw.length >> 1;
  const rms = new Float32Array(n);
  const peak = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    rms[i] = raw[2 * i] / 127;
    peak[i] = raw[2 * i + 1] / 127;
  }
  return { perSec: view.getUint32(0, true), duration: view.getFloat64(8, true), rms, peak };
}

/** Error text of a request that a newer one superseded (not a failure). */
export const CANCELLED = "cancelled";

export const api = {
  ping: () => invoke<string>("ping"),
  appInfo: () => invoke<AppInfo>("app_info"),
  analyzerInfo: () => invoke<AnalyzerInfo>("analyzer_info"),

  pickFolder: (title?: string) => invoke<string | null>("pick_folder", { title }),
  openSession: (path: string) => invoke<Session>("open_session", { path }),
  recentSessions: () => invoke<RecentSession[]>("recent_sessions"),
  setIncludeSubfolders: (sessionId: number, value: boolean) =>
    invoke<Session>("set_include_subfolders", { sessionId, value }),
  listTracks: (sessionId: number) => invoke<Track[]>("list_tracks", { sessionId }),
  scanSession: (sessionId: number) => invoke<ScanSummary>("scan_session", { sessionId }),
  setStartAt: (sessionId: number, value: number) =>
    invoke<Session>("set_start_at", { sessionId, value }),
  setCurrentTrack: (sessionId: number, trackId: number | null) =>
    invoke<null>("set_current_track", { sessionId, trackId }),
  setMode: (sessionId: number, mode: Session["mode"]) =>
    invoke<Session>("set_mode", { sessionId, mode }),
  setAutoNext: (sessionId: number, value: boolean) =>
    invoke<Session>("set_auto_next", { sessionId, value }),

  listDestinations: (sessionId: number) =>
    invoke<Destination[]>("list_destinations", { sessionId }),
  addDestination: (sessionId: number, path: string) =>
    invoke<Destination[]>("add_destination", { sessionId, path }),
  removeDestination: (sessionId: number, destinationId: number) =>
    invoke<Destination[]>("remove_destination", { sessionId, destinationId }),
  setDestinationHotkey: (sessionId: number, destinationId: number, hotkey: number | null) =>
    invoke<Destination[]>("set_destination_hotkey", { sessionId, destinationId, hotkey }),

  decide: (sessionId: number, trackId: number, decision: Decision) =>
    invoke<Outcome>("decide", { sessionId, trackId, decision }),
  undo: (sessionId: number) => invoke<Undone | null>("undo", { sessionId }),

  watchSession: (sessionId: number) => invoke<null>("watch_session", { sessionId }),
  unwatchSession: () => invoke<null>("unwatch_session"),
  revealTrack: (sessionId: number, trackId: number) =>
    invoke<null>("reveal_track", { sessionId, trackId }),
  trackApps: (sessionId: number, trackId: number) =>
    invoke<OpenWithApp[]>("track_apps", { sessionId, trackId }),
  openTrack: (sessionId: number, trackId: number, appPath: string | null) =>
    invoke<null>("open_track", { sessionId, trackId, appPath }),

  editTrack: (sessionId: number, trackId: number, field: EditField, value: string) =>
    invoke<Track>("edit_track", { sessionId, trackId, field, value }),

  trackPeaks: (trackId: number) =>
    invoke<ArrayBuffer>("track_peaks", { trackId }).then(parsePeaks),
};

export const events = {
  onScanProgress: (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan_progress", (e) => cb(e.payload)),
  /** Something changed in the session's folder outside the app (Finder). */
  onFolderChanged: (cb: (p: FolderChanged) => void): Promise<UnlistenFn> =>
    listen<FolderChanged>("folder_changed", (e) => cb(e.payload)),
};
