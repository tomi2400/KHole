import { TriangleAlert, Trash2 } from "lucide-react";
import type { Destination } from "@/lib/api";
import { fileName } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useDrag } from "@/state/drag";

/** Which tile to flash; `n` changes on every use so the same tile can flash again. */
export interface Flash {
  tile: number | "trash";
  n: number;
  warning?: boolean;
}

/** A line in the bar. Warnings (duplicate, renamed, errors) are a banner that is hard to miss. */
export interface Notice {
  text: string;
  tone: "info" | "warning";
}

interface Props {
  destinations: Destination[];
  trashed: number;
  mode: "move" | "copy";
  flash: Flash | null;
  /** Last used tile keeps the accent until another one is used. */
  lastUsed: number | "trash" | null;
  notice: Notice | null;
  onSort: (destinationId: number) => void;
  onTrash: () => void;
  onMoveTo: () => void;
}

/**
 * Destination tiles with their number keys and counters, plus the Trash tile. Feedback is a
 * short flash and the counter, not a toast. Tracks can be dragged from the list onto any
 * tile (`data-drop`, see `state/drag.ts`). Folders without a number key share one
 * "Move to…" tile, which opens the search (M).
 */
export function DestinationBar({
  destinations,
  trashed,
  mode,
  flash,
  lastUsed,
  notice,
  onSort,
  onTrash,
  onMoveTo,
}: Props) {
  const dragging = useDrag((s) => s.track != null);
  const over = useDrag((s) => s.over);
  const keyed = destinations.filter((d) => d.hotkey != null);
  const unkeyed = destinations.filter((d) => d.hotkey == null);
  const isUnkeyed = (tile: number | "trash" | null | undefined) =>
    unkeyed.some((d) => d.id === tile);

  return (
    <footer className="flex shrink-0 flex-col border-t border-border">
      {notice?.tone === "warning" ? (
        <div
          role="alert"
          className="mb-2 flex h-8 items-center gap-2 border-b border-warning/40 bg-warning/15 px-3 text-sm font-medium text-warning"
        >
          <TriangleAlert className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate" title={notice.text}>
            {notice.text}
          </span>
        </div>
      ) : (
        <div className="flex h-10 items-center gap-2 px-3 text-[11px] text-muted-foreground">
          <span>
            {destinations.length === 0
              ? "No destinations – cleanup mode: Enter keeps, Backspace trashes"
              : `${mode === "copy" ? "Copying" : "Moving"} · 1–0 sorts · M finds a folder · Enter keeps · Backspace trashes · ⌘Z undoes · ? all keys`}
          </span>
          {notice && (
            <span className="ml-auto truncate text-foreground" role="status">
              {notice.text}
            </span>
          )}
        </div>
      )}
      <div className="flex gap-2 overflow-x-auto px-3 pb-3">
        {keyed.map((d) => (
          <Tile
            key={d.id}
            title={d.path}
            drop={`dest:${d.id}`}
            dragging={dragging}
            over={over === `dest:${d.id}`}
            onClick={() => onSort(d.id)}
            active={lastUsed === d.id}
            flashKey={flash?.tile === d.id ? flash.n : null}
            warning={!!flash?.warning}
          >
            <kbd className="tabular flex size-5 shrink-0 items-center justify-center rounded bg-muted text-xs font-semibold">
              {d.hotkey ?? "·"}
            </kbd>
            <span className="min-w-0 flex-1 truncate">{fileName(d.path)}</span>
            <span className="tabular shrink-0 text-xs text-muted-foreground">{d.count || ""}</span>
          </Tile>
        ))}
        {unkeyed.length > 0 && (
          <Tile
            title={`Move to… – ${unkeyed.length} more ${unkeyed.length === 1 ? "folder" : "folders"} without a number key (M)`}
            drop="moveTo"
            dragging={dragging}
            over={over === "moveTo"}
            onClick={onMoveTo}
            active={isUnkeyed(lastUsed)}
            flashKey={isUnkeyed(flash?.tile) ? flash!.n : null}
            warning={!!flash?.warning}
          >
            <kbd className="flex size-5 shrink-0 items-center justify-center rounded bg-muted text-xs font-semibold">
              M
            </kbd>
            <span className="min-w-0 flex-1 truncate">Move to…</span>
            <span className="tabular shrink-0 text-xs text-muted-foreground">
              +{unkeyed.length}
            </span>
          </Tile>
        )}
        <Tile
          title="Move to Trash (Backspace)"
          drop="trash"
          dragging={dragging}
          over={over === "trash"}
          onClick={onTrash}
          active={lastUsed === "trash"}
          flashKey={flash?.tile === "trash" ? flash.n : null}
          warning={!!flash?.warning}
          className="ml-auto"
        >
          <Trash2 className="size-4 shrink-0 text-muted-foreground" />
          <span className="flex-1">Trash</span>
          <span className="tabular shrink-0 text-xs text-muted-foreground">{trashed || ""}</span>
        </Tile>
      </div>
    </footer>
  );
}

function Tile({
  title,
  drop,
  dragging,
  over,
  onClick,
  active,
  flashKey,
  warning,
  className,
  children,
}: {
  title: string;
  /** `data-drop` value: tracks dragged from the list can be dropped here. */
  drop: string;
  /** A track is being dragged: tiles show they take it. */
  dragging: boolean;
  /** The dragged track is over this tile. */
  over: boolean;
  onClick: () => void;
  active: boolean;
  flashKey: number | null;
  warning: boolean;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      data-drop={drop}
      onClick={onClick}
      // Keys belong to the keymap; a focused tile must not turn Space or Enter into a click.
      tabIndex={-1}
      className={cn(
        "relative flex h-10 w-44 shrink-0 items-center gap-2 overflow-hidden rounded-lg border bg-card px-2.5 text-left text-sm",
        over
          ? "border-brand bg-brand/20"
          : dragging
            ? "border-dashed border-muted-foreground/60"
            : active
              ? "border-brand"
              : "border-border hover:bg-accent/60",
        className,
      )}
    >
      {flashKey != null && (
        <span
          key={flashKey}
          className={cn(
            "pointer-events-none absolute inset-0",
            warning ? "tile-flash-warning bg-warning" : "tile-flash bg-brand",
          )}
        />
      )}
      {children}
    </button>
  );
}
