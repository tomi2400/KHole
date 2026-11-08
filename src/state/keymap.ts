import { useEffect, useRef } from "react";

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
  | "undo"
  | "reveal";

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
  /** For the shortcut overview (M6). */
  label: string;
}

export const KEYMAP: readonly Binding[] = [
  { code: "Space", action: "playPause", label: "Play / pause" },
  { key: "ArrowLeft", repeat: true, action: "seekBack", label: "Back 15 s" },
  { key: "ArrowRight", repeat: true, action: "seekForward", label: "Forward 15 s" },
  { key: "ArrowLeft", shift: true, repeat: true, action: "seekBackLong", label: "Back 60 s" },
  {
    key: "ArrowRight",
    shift: true,
    repeat: true,
    action: "seekForwardLong",
    label: "Forward 60 s",
  },
  { key: "ArrowUp", repeat: true, action: "prevTrack", label: "Previous track" },
  { key: "ArrowDown", repeat: true, action: "nextTrack", label: "Next track" },
  { key: "Enter", action: "keep", label: "Keep, next track" },
  { key: "Backspace", action: "trash", label: "Move to Trash, next track" },
  // `key`, not `code`: on a Hungarian keyboard the 0 sits left of the 1.
  ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map(
    (n): Binding => ({ key: String(n), arg: n, action: "sort", label: `Sort into destination ${n}` }),
  ),
  { key: "z", meta: true, action: "undo", label: "Undo" },
  { key: "r", meta: true, action: "reveal", label: "Reveal in Finder" },
];

export const SEEK_SHORT = 15;
export const SEEK_LONG = 60;

export type Handlers = Partial<Record<Action, (arg?: number) => void>>;

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
      if (isTextInput(e.target)) return;
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
