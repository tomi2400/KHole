import { convertFileSrc } from "@tauri-apps/api/core";
import { create } from "zustand";
import type { Track } from "@/lib/api";

/**
 * The one audio element of the app. Playback is WebKit's (asset protocol streams the file);
 * the waveform (wavesurfer) attaches to this element instead of owning one.
 */
export const audio = new Audio();
audio.preload = "auto";

interface PlayerState {
  trackId: number | null;
  playing: boolean;
  /** Seconds; refreshed on `timeupdate` (≈4×/s), enough for the clock. */
  time: number;
  /** Seconds, from the media element once loaded (more exact than the tag). */
  duration: number;
  error: string | null;
}

export const usePlayer = create<PlayerState>(() => ({
  trackId: null,
  playing: false,
  time: 0,
  duration: 0,
  error: null,
}));

const set = usePlayer.setState;
let loadToken = 0;

audio.addEventListener("play", () => set({ playing: true }));
audio.addEventListener("pause", () => set({ playing: false }));
audio.addEventListener("timeupdate", () => set({ time: audio.currentTime }));
audio.addEventListener("seeking", () => set({ time: audio.currentTime }));
audio.addEventListener("durationchange", () =>
  set({ duration: Number.isFinite(audio.duration) ? audio.duration : 0 }),
);
audio.addEventListener("error", () => {
  if (audio.getAttribute("src")) set({ error: "This file can't be played.", playing: false });
});

export interface LoadOptions {
  autoplay: boolean;
  /** Fraction of the track to start at (session `startAt`). */
  startAt: number;
}

/** Loads a track; the start position is applied once its length is known. */
export function loadTrack(track: Track, { autoplay, startAt }: LoadOptions): void {
  const token = ++loadToken;
  audio.pause();
  audio.src = convertFileSrc(track.path);
  set({ trackId: track.id, time: 0, duration: track.duration ?? 0, error: null });

  audio.addEventListener(
    "loadedmetadata",
    () => {
      if (token !== loadToken) return;
      if (startAt > 0 && Number.isFinite(audio.duration)) {
        audio.currentTime = audio.duration * startAt;
      }
      if (autoplay) void audio.play().catch(() => {});
    },
    { once: true },
  );
}

export function togglePlay(): void {
  if (!audio.getAttribute("src") || usePlayer.getState().error) return;
  if (audio.paused) void audio.play().catch(() => {});
  else audio.pause();
}

export function seekTo(seconds: number): void {
  const d = audio.duration;
  if (!Number.isFinite(d)) return;
  audio.currentTime = Math.min(Math.max(seconds, 0), d);
}

export function seekBy(delta: number): void {
  seekTo(audio.currentTime + delta);
}

export function stop(): void {
  ++loadToken;
  audio.pause();
  audio.removeAttribute("src");
  audio.load();
  set({ trackId: null, playing: false, time: 0, duration: 0, error: null });
}
