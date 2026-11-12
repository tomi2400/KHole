import type { PointerEvent as ReactPointerEvent } from "react";
import { create } from "zustand";
import type { Track } from "@/lib/api";

/**
 * Dragging a track from the list onto a destination tile. Pointer events, not HTML5
 * drag and drop: the window's native file drop (open a folder) owns the HTML5 drag
 * events in the WebView, and a pointer drag needs no drag image or data transfer.
 *
 * Drop targets are elements with `data-drop`: "dest:<id>", "trash" or "moveTo".
 */
export type DropTarget = { kind: "dest"; id: number } | { kind: "trash" } | { kind: "moveTo" };

interface DragState {
  /** The track being dragged, once the pointer moved far enough. */
  track: Track | null;
  x: number;
  y: number;
  /** `data-drop` value under the pointer. */
  over: string | null;
}

export const useDrag = create<DragState>(() => ({ track: null, x: 0, y: 0, over: null }));

/** Pixels the pointer has to move before a press becomes a drag (a click stays a click). */
export const DRAG_THRESHOLD = 4;

export function parseDropTarget(value: string | null): DropTarget | null {
  if (value === "trash") return { kind: "trash" };
  if (value === "moveTo") return { kind: "moveTo" };
  const m = value?.match(/^dest:(\d+)$/);
  return m ? { kind: "dest", id: Number(m[1]) } : null;
}

function dropAt(x: number, y: number): string | null {
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]");
  return el?.dataset.drop ?? null;
}

/**
 * Call from a row's `onPointerDown`. Becomes a drag after a few pixels; dropping on a
 * target calls `onDrop`. Esc, losing the window or releasing elsewhere cancels.
 */
export function beginTrackDrag(
  e: ReactPointerEvent,
  track: Track,
  onDrop: (track: Track, target: DropTarget) => void,
): void {
  if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const startX = e.clientX;
  const startY = e.clientY;
  let dragging = false;

  const move = (ev: PointerEvent) => {
    if (!dragging) {
      if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return;
      dragging = true;
      document.documentElement.classList.add("dragging-track");
    }
    useDrag.setState({ track, x: ev.clientX, y: ev.clientY, over: dropAt(ev.clientX, ev.clientY) });
  };

  const finish = (ev: PointerEvent | null) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("keydown", key, { capture: true });
    window.removeEventListener("blur", cancel);
    if (!dragging) return;
    document.documentElement.classList.remove("dragging-track");
    // The click that ends a drag must not also select a row or start editing a cell.
    swallowNextClick();
    const target = ev ? parseDropTarget(dropAt(ev.clientX, ev.clientY)) : null;
    useDrag.setState({ track: null, over: null });
    if (target) onDrop(track, target);
  };
  const up = (ev: PointerEvent) => finish(ev);
  const cancel = () => finish(null);
  const key = (ev: KeyboardEvent) => {
    if (!dragging) return;
    // While dragging, keys do not reach the triage; Esc cancels.
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.key === "Escape") finish(null);
  };

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("keydown", key, { capture: true });
  window.addEventListener("blur", cancel);
}

function swallow(e: MouseEvent): void {
  e.stopPropagation();
  e.preventDefault();
}

/** Call on the pointerup that ends a drag: the click it produces is dropped. */
export function swallowNextClick(): void {
  window.addEventListener("click", swallow, { capture: true, once: true });
  // No click follows when the pointer was released elsewhere; do not eat a later one.
  window.setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
}
