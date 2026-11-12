import { useEffect, useRef } from "react";
import { useDrag } from "@/state/drag";

/**
 * Central keymap: every shortcut of the app is listed here, and one window listener
 * dispatches them. Screens only say what an action does (`useKeymap`). Remapping (spec #14)
 * will edit this table.
 */
export type Action =
  | "playPause"
  | "seekBack"
  | "seekForward"
  | "seekBackLong"
  | "seekForwardLong"
  | "prevTrack"
  | "nextTrack"
  | "keep"
  | "trash"
  /** Sort into the destination whose number key is `arg` (1–9, 0). */
  | "sort"
  | "moveTo"
  | "undo"
  | "reveal"
  | "shortcuts";

/** Sections of the shortcut overview (`?`), in display order. */
export const SECTIONS = ["Playback", "Tracks", "Decide", "Other"] as const;
export type Section = (typeof SECTIONS)[number];

interface Binding {
  /** `KeyboardEvent.code` for layout-independent keys (Space), otherwise `key`. */
  code?: string;
  key?: string;
  shift?: boolean;
  /** ⌘ held. */
  meta?: boolean;
  /** Passed to the handler (the number of a destination key). */
  arg?: number;
  /** Fire again while the key is held. */
  repeat?: boolean;
  action: Action;
  /** For the shortcut overview (`?`). */
  label: string;
  section: Section;
  /** Key text in the overview instead of the one derived from the binding. */
  display?: string;
  /** Left out of the overview (covered by another row, like 2–0 by "1–0"). */
  hidden?: boolean;
}

export const KEYMAP: readonly Binding[] = [
  { code: "Space", action: "playPause", label: "Play / pause", section: "Playback" },
  { key: "ArrowLeft", repeat: true, action: "seekBack", label: "Back 15 s", section: "Playback" },
  {
    key: "ArrowRight",
    repeat: true,
    action: "seekForward",
    label: "Forward 15 s",
    section: "Playback",
  },
  {
    key: "ArrowLeft",
    shift: true,
    repeat: true,
    action: "seekBackLong",
    label: "Back 60 s",
    section: "Playback",
  },
  {
    key: "ArrowRight",
    shift: true,
    repeat: true,
    action: "seekForwardLong",
    label: "Forward 60 s",
    section: "Playback",
  },
  {
    key: "ArrowUp",
    repeat: true,
    action: "prevTrack",
    label: "Previous track (no decision)",
    section: "Tracks",
  },
  {
    key: "ArrowDown",
    repeat: true,
    action: "nextTrack",
    label: "Next track (no decision)",
    section: "Tracks",
  },
  // `key`, not `code`: on a Hungarian keyboard the 0 sits left of the 1.
  ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map(
    (n): Binding => ({
      key: String(n),
      arg: n,
      action: "sort",
      label: "Sort into that destination, next track",
      section: "Decide",
      display: "1–0",
      hidden: n !== 1,
    }),
  ),
  { key: "m", action: "moveTo", label: "Move to… (find any destination)", section: "Decide" },
  { key: "Enter", action: "keep", label: "Keep where it is, next track", section: "Decide" },
  { key: "Backspace", action: "trash", label: "Move to Trash, next track", section: "Decide" },
  { key: "z", meta: true, action: "undo", label: "Undo", section: "Decide" },
  { key: "r", meta: true, action: "reveal", label: "Reveal in Finder", section: "Other" },
  { key: "?", shift: true, action: "shortcuts", label: "This overview", section: "Other" },
];

const KEY_TEXT: Record<string, string> = {
  Space: "Space",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Enter: "Enter",
  Backspace: "Backspace",
};

/** The keys of a binding as shown to the user, e.g. ["⇧", "←"]. */
export function keyCaps(b: Binding): string[] {
  if (b.display) return [b.display];
  const name = b.code ?? b.key ?? "";
  const main = KEY_TEXT[name] ?? name.toUpperCase();
  // "?" already implies Shift.
  return [...(b.meta ? ["⌘"] : []), ...(b.shift && name !== "?" ? ["⇧"] : []), main];
}

export const SEEK_SHORT = 15;
export const SEEK_LONG = 60;

export type Handlers = Partial<Record<Action, (arg?: number) => void>>;

/** Keys typed into a dialog (Move to…, the overview) belong to it, not to the triage. */
function inDialog(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('[role="dialog"]') != null;
}

function isTextInput(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  return (
    target instanceof HTMLInputElement &&
    !["checkbox", "radio", "button", "range"].includes(target.type)
  );
}

export function match(e: KeyboardEvent): Binding | undefined {
  if (e.ctrlKey || e.altKey || e.isComposing) return undefined;
  return KEYMAP.find(
    (b) =>
      (b.code ? b.code === e.code : b.key === e.key.toLowerCase() || b.key === e.key) &&
      !!b.shift === e.shiftKey &&
      !!b.meta === e.metaKey &&
      (b.repeat || !e.repeat),
  );
}

/**
 * Installs the handlers of the current screen. Keys are taken in the capture phase so a
 * focused button or switch does not also react to Space or the arrows; text fields keep
 * their keys.
 */
export function useKeymap(handlers: Handlers): void {
  const ref = useRef(handlers);
  ref.current = handlers;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTextInput(e.target) || inDialog(e.target)) return;
      // A track is being dragged: Esc cancels the drag, nothing else acts meanwhile.
      if (useDrag.getState().track) return;
      const binding = match(e);
      const run = binding && ref.current[binding.action];
      if (!run) return;
      e.preventDefault();
      e.stopPropagation();
      run(binding.arg);
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, []);
}
