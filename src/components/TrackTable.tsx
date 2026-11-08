import { useEffect, useMemo, useRef, useState } from "react";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type RowData,
  type SortingState,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { cn } from "@/lib/utils";
import type { EditField, Track, TrackStatus } from "@/lib/api";
import { formatBpm, formatDuration, stem } from "@/lib/format";

declare module "@tanstack/react-table" {
  interface ColumnMeta<TData extends RowData, TValue> {
    /** CSS grid track size. */
    width: string;
    numeric?: boolean;
  }
}

type Row = Track & { index: number };

const ROW_HEIGHT = 28;

const STATUS_LABEL: Record<TrackStatus, string> = {
  pending: "",
  kept: "Kept",
  moved: "Moved",
  copied: "Copied",
  trashed: "Trashed",
  missing: "Missing",
};

/** Columns edited in place (click a cell of the selected row), and what they edit. */
const EDITABLE: Record<string, EditField> = { title: "title", artist: "artist", file: "file" };
/** Statuses whose file can be edited (not in the Trash, not gone). */
const EDITABLE_STATUS: TrackStatus[] = ["pending", "kept", "copied", "moved"];
/** A second click waits this long, so a double click (play) does not start editing. */
const EDIT_DELAY_MS = 450;

function editValue(track: Track, field: EditField): string {
  if (field === "title") return track.title ?? "";
  if (field === "artist") return track.artist ?? "";
  return stem(track.path);
}

const col = createColumnHelper<Row>();

// `undefined` (not null) so TanStack's `sortUndefined: "last"` keeps empty cells at the bottom.
const columns = [
  col.accessor("index", {
    header: "#",
    meta: { width: "3rem", numeric: true },
  }),
  col.accessor((r) => r.title ?? stem(r.path), {
    id: "title",
    header: "Title",
    sortingFn: "alphanumeric",
    meta: { width: "minmax(8rem, 3fr)" },
  }),
  // The file name (renames in Finder show up here; the title comes from the tags).
  col.accessor((r) => stem(r.path), {
    id: "file",
    header: "File",
    sortingFn: "alphanumeric",
    meta: { width: "minmax(8rem, 2fr)" },
  }),
  col.accessor((r) => r.artist ?? undefined, {
    id: "artist",
    header: "Artist",
    sortingFn: "alphanumeric",
    sortUndefined: "last",
    meta: { width: "minmax(6rem, 2fr)" },
  }),
  col.accessor((r) => r.bpm ?? undefined, {
    id: "bpm",
    header: "BPM",
    sortUndefined: "last",
    cell: (c) => formatBpm(c.getValue() ?? null),
    meta: { width: "4rem", numeric: true },
  }),
  col.accessor((r) => r.musicalKey ?? undefined, {
    id: "key",
    header: "Key",
    sortingFn: "alphanumeric",
    sortUndefined: "last",
    meta: { width: "3.5rem" },
  }),
  col.accessor((r) => r.duration ?? undefined, {
    id: "time",
    header: "Time",
    sortUndefined: "last",
    cell: (c) => formatDuration(c.getValue() ?? null),
    meta: { width: "4rem", numeric: true },
  }),
  col.accessor((r) => r.format ?? undefined, {
    id: "format",
    header: "Format",
    sortUndefined: "last",
    meta: { width: "4rem" },
  }),
  col.accessor((r) => r.bitrate ?? undefined, {
    id: "bitrate",
    header: "kbps",
    sortUndefined: "last",
    meta: { width: "3.5rem", numeric: true },
  }),
  col.accessor("status", {
    header: "Status",
    cell: (c) => STATUS_LABEL[c.getValue()],
    meta: { width: "4.5rem" },
  }),
];

interface Props {
  tracks: Track[];
  currentId: number | null;
  /** Single click: make it the current track. */
  onSelect: (track: Track) => void;
  /** Double click: load and play. */
  onActivate: (track: Track) => void;
  /** Track ids in display (sorted) order; ↑/↓ step through this. */
  onOrderChange: (ids: number[]) => void;
  /** Right click: the track's menu. */
  onContextMenu: (track: Track) => void;
  /** An edited cell was committed with a changed value. */
  onEdit: (track: Track, field: EditField, value: string) => void;
}

/** The text field of a cell being edited. Enter or leaving it saves, Esc cancels. */
function EditCell({
  initial,
  placeholder,
  onDone,
}: {
  initial: string;
  placeholder?: string;
  onDone: (value: string | null) => void;
}) {
  const done = useRef(false);
  const finish = (value: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(value);
  };
  return (
    <input
      autoFocus
      defaultValue={initial}
      placeholder={placeholder}
      spellCheck={false}
      onFocus={(e) => e.currentTarget.select()}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(e.currentTarget.value);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.currentTarget.value)}
      className="h-6 w-full min-w-0 rounded-sm border border-brand bg-background px-1 text-[13px] text-foreground outline-none"
    />
  );
}

/** Dense, sortable, virtualized track list (DJ-browser style, 28 px rows). */
export function TrackTable({
  tracks,
  currentId,
  onSelect,
  onActivate,
  onOrderChange,
  onContextMenu,
  onEdit,
}: Props) {
  const [editing, setEditing] = useState<{ id: number; field: EditField } | null>(null);
  const editTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(editTimer.current), []);
  // Another track becoming current ends editing.
  useEffect(() => {
    setEditing((e) => (e && e.id !== currentId ? null : e));
  }, [currentId]);

  const data = useMemo<Row[]>(() => tracks.map((t, i) => ({ ...t, index: i + 1 })), [tracks]);
  const [sorting, setSorting] = useState<SortingState>([]);
  const table = useReactTable({
    data,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getRowId: (r) => String(r.id),
  });
  const rows = table.getRowModel().rows;

  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 16,
  });

  useEffect(() => {
    onOrderChange(rows.map((r) => r.original.id));
  }, [rows, onOrderChange]);

  // Keep the current track in view when it changes (↑/↓, resume), not on every re-sort.
  const currentIndex = currentId == null ? -1 : rows.findIndex((r) => r.original.id === currentId);
  useEffect(() => {
    if (currentIndex >= 0) virtualizer.scrollToIndex(currentIndex, { align: "auto" });
  }, [currentId]); // eslint-disable-line react-hooks/exhaustive-deps

  const gridTemplateColumns = table
    .getVisibleLeafColumns()
    .map((c) => c.columnDef.meta?.width ?? "1fr")
    .join(" ");

  return (
    <div ref={scrollRef} role="table" className="min-h-0 flex-1 overflow-auto text-[13px]">
      <div role="rowgroup" className="sticky top-0 z-10 border-b border-border bg-background">
        {table.getHeaderGroups().map((hg) => (
          <div key={hg.id} role="row" className="grid gap-3 px-3" style={{ gridTemplateColumns }}>
            {hg.headers.map((h) => {
              const sorted = h.column.getIsSorted();
              const numeric = h.column.columnDef.meta?.numeric;
              return (
                <button
                  key={h.id}
                  role="columnheader"
                  aria-sort={
                    sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : "none"
                  }
                  onClick={h.column.getToggleSortingHandler()}
                  className={cn(
                    "flex h-7 min-w-0 items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground",
                    numeric && "justify-end",
                  )}
                >
                  <span className="truncate">
                    {flexRender(h.column.columnDef.header, h.getContext())}
                  </span>
                  {sorted === "asc" && <ArrowUp className="size-3 shrink-0" />}
                  {sorted === "desc" && <ArrowDown className="size-3 shrink-0" />}
                </button>
              );
            })}
          </div>
        ))}
      </div>

      <div role="rowgroup" className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((v) => {
          const row = rows[v.index];
          const current = row.original.id === currentId;
          return (
            <div
              key={row.id}
              role="row"
              aria-selected={current}
              onClick={() => onSelect(row.original)}
              onDoubleClick={() => {
                window.clearTimeout(editTimer.current);
                onActivate(row.original);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                onContextMenu(row.original);
              }}
              className={cn(
                "absolute inset-x-0 grid cursor-default items-center gap-3 px-3 select-none",
                current
                  ? "bg-brand/20 shadow-[inset_2px_0_0_var(--brand)]"
                  : cn("hover:bg-accent/60", v.index % 2 === 1 && "bg-muted/30"),
                row.original.status !== "pending" && !current && "opacity-45",
              )}
              style={{
                gridTemplateColumns,
                height: ROW_HEIGHT,
                transform: `translateY(${v.start}px)`,
              }}
            >
              {row.getVisibleCells().map((cell) => {
                const meta = cell.column.columnDef.meta;
                const content = flexRender(cell.column.columnDef.cell, cell.getContext());
                const text = cell.getValue();
                const field = EDITABLE[cell.column.id];
                const track = row.original;
                const canEdit = field && current && EDITABLE_STATUS.includes(track.status);
                if (canEdit && editing?.id === track.id && editing.field === field) {
                  const initial = editValue(track, field);
                  return (
                    <div key={cell.id} role="cell" className="min-w-0">
                      <EditCell
                        initial={initial}
                        placeholder={field === "title" ? stem(track.path) : undefined}
                        onDone={(value) => {
                          setEditing(null);
                          if (value != null && value.trim() !== initial.trim()) {
                            onEdit(track, field, value);
                          }
                        }}
                      />
                    </div>
                  );
                }
                return (
                  <div
                    key={cell.id}
                    role="cell"
                    onClick={
                      canEdit
                        ? () => {
                            // A click on the selected row's cell edits it (as in Finder).
                            window.clearTimeout(editTimer.current);
                            editTimer.current = window.setTimeout(
                              () => setEditing({ id: track.id, field }),
                              EDIT_DELAY_MS,
                            );
                          }
                        : undefined
                    }
                    title={
                      canEdit
                        ? `${typeof text === "string" ? `${text} – ` : ""}click to edit`
                        : typeof text === "string"
                          ? text
                          : undefined
                    }
                    className={cn(
                      "truncate",
                      meta?.numeric && "tabular text-right",
                      cell.column.id !== "title" && !current && "text-muted-foreground",
                    )}
                  >
                    {content}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
