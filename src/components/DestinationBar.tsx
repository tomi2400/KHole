import { TriangleAlert, Trash2 } from "lucide-react";
import type { Destination } from "@/lib/api";
import { fileName } from "@/lib/format";
import { cn } from "@/lib/utils";

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
}

/**
 * Destination tiles with their number keys and counters, plus the Trash tile. Feedback is a
 * short flash and the counter, not a toast. Dragging tracks onto tiles comes in M6.
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
}: Props) {
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
              : `${mode === "copy" ? "Copying" : "Moving"} · 1–0 sorts · Enter keeps · Backspace trashes · ⌘Z undoes`}
          </span>
          {notice && (
            <span className="ml-auto truncate text-foreground" role="status">
              {notice.text}
            </span>
          )}
        </div>
      )}
      <div className="flex gap-2 overflow-x-auto px-3 pb-3">
        {destinations.map((d) => (
          <Tile
            key={d.id}
            title={d.path}
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
        <Tile
          title="Move to Trash (Backspace)"
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
  onClick,
  active,
  flashKey,
  warning,
  className,
  children,
}: {
  title: string;
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
      onClick={onClick}
      // Keys belong to the keymap; a focused tile must not turn Space or Enter into a click.
      tabIndex={-1}
      className={cn(
        "relative flex h-10 w-44 shrink-0 items-center gap-2 overflow-hidden rounded-lg border bg-card px-2.5 text-left text-sm",
        active ? "border-brand" : "border-border hover:bg-accent/60",
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
