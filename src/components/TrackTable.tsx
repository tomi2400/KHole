import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type RowData,
  type SortingState,
  type VisibilityState,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { cn } from "@/lib/utils";
import type { EditField, Track, TrackStatus } from "@/lib/api";
import { formatBpm, formatDuration, stem } from "@/lib/format";
import { isStringArray, loadPref, removePref, savePref } from "@/lib/prefs";
import { showColumnsMenu } from "@/lib/trackMenu";
import { DRAG_THRESHOLD, beginTrackDrag, swallowNextClick, type DropTarget } from "@/state/drag";

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
const EDITABLE: Record<string, EditField> = {
  title: "title",
  artist: "artist",
  file: "file",
  bpm: "bpm",
};
/** Statuses whose file can be edited (not in the Trash, not gone). */
const EDITABLE_STATUS: TrackStatus[] = ["pending", "kept", "copied", "moved"];
/** Statuses a decision can be made on (their file is still in the source): draggable. */
const DECIDABLE_STATUS: TrackStatus[] = ["pending", "kept", "copied"];
/** A second click waits this long, so a double click (play) does not start editing. */
const EDIT_DELAY_MS = 450;

function editValue(track: Track, field: EditField): string {
  if (field === "title") return track.title ?? "";
  if (field === "artist") return track.artist ?? "";
  if (field === "bpm") return track.bpm == null ? "" : String(Math.round(track.bpm * 100) / 100);
  return stem(track.path);
}

/**
 * A value not from the file's tags: KHole's analysis is a shade lighter, a BPM set by hand
 * is italic. Both with a tooltip.
 */
function Source({ of, children }: { of: Track["bpmSource"]; children: ReactNode }) {
  if (of === "analysis") {
    return (
      <span className="opacity-60" title="Analyzed by KHole (not in the file's tags)">
        {children}
      </span>
    );
  }
  if (of === "manual") {
    return (
      <span className="italic" title="Set by hand in KHole (the file is unchanged)">
        {children}
      </span>
    );
  }
  return children;
}

const col = createColumnHelper<Row>();

// `undefined` (not null) so TanStack's `sortUndefined: "last"` keeps empty cells at the bottom.
const columns = [
  col.accessor("index", {
    id: "index",
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
    cell: (c) => (
      <Source of={c.row.original.bpmSource}>{formatBpm(c.getValue() ?? null)}</Source>
    ),
    meta: { width: "4rem", numeric: true },
  }),
  col.accessor((r) => r.musicalKey ?? undefined, {
    id: "key",
    header: "Key",
    sortingFn: "alphanumeric",
    sortUndefined: "last",
    cell: (c) => (
      <Source of={c.row.original.keyAnalyzed ? "analysis" : null}>{c.getValue()}</Source>
    ),
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
    id: "status",
    header: "Status",
    cell: (c) => STATUS_LABEL[c.getValue()],
    meta: { width: "4.5rem" },
  }),
];

const COLUMN_IDS = columns.map((c) => c.id!);
/** Names in the header's right-click menu. */
const COLUMN_LABEL: Record<string, string> = { index: "# (folder order)", time: "Time", bitrate: "Bitrate (kbps)" };
const columnLabel = (id: string): string =>
  COLUMN_LABEL[id] ?? String(columns.find((c) => c.id === id)?.header ?? id);
/** Always shown: a row needs a name. */
const LOCKED_COLUMN = "title";

const ORDER_PREF = "columns.order";
const HIDDEN_PREF = "columns.hidden";

/** Saved order, with unknown ids dropped and new columns appended. */
function loadOrder(): string[] {
  const saved = loadPref<string[]>(ORDER_PREF, [], isStringArray).filter((id) =>
    COLUMN_IDS.includes(id),
  );
  return [...saved, ...COLUMN_IDS.filter((id) => !saved.includes(id))];
}

function loadHidden(): VisibilityState {
  const hidden = loadPref<string[]>(HIDDEN_PREF, [], isStringArray);
  return Object.fromEntries(
    hidden.filter((id) => COLUMN_IDS.includes(id) && id !== LOCKED_COLUMN).map((id) => [id, false]),
  );
}

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
  /** A row was dragged onto a destination tile, the Trash or Move to…. */
  onDropTrack: (track: Track, target: DropTarget) => void;
  /** Only tracks still to do are listed (and the current one, until you move on). */
  hideDone: boolean;
  onShowDone: () => void;
  /** The session's saved sort order (JSON, see `encodeSort`); null: folder order. */
  initialSort: string | null;
  onSortChange: (sort: string | null) => void;
}

/** A saved sort order, dropping anything that is not a current, sortable column. */
function decodeSort(saved: string | null): SortingState {
  if (!saved) return [];
  try {
    const value: unknown = JSON.parse(saved);
    if (!Array.isArray(value)) return [];
    const ids = new Set(columns.map((c) => c.id));
    return value.filter(
      (s): s is { id: string; desc: boolean } =>
        typeof s === "object" &&
        s != null &&
        typeof s.id === "string" &&
        typeof s.desc === "boolean" &&
        ids.has(s.id),
    );
  } catch {
    return [];
  }
}

function encodeSort(sorting: SortingState): string | null {
  return sorting.length ? JSON.stringify(sorting.map(({ id, desc }) => ({ id, desc }))) : null;
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
      // Selecting text in the field must not drag the row.
      onPointerDown={(e) => e.stopPropagation()}
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
  onDropTrack,
  hideDone,
  onShowDone,
  initialSort,
  onSortChange,
}: Props) {
  const [editing, setEditing] = useState<{ id: number; field: EditField } | null>(null);
  const editTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(editTimer.current), []);
  // Another track becoming current ends editing.
  useEffect(() => {
    setEditing((e) => (e && e.id !== currentId ? null : e));
  }, [currentId]);

  const data = useMemo<Row[]>(() => tracks.map((t, i) => ({ ...t, index: i + 1 })), [tracks]);
  const [sorting, setSorting] = useState<SortingState>(() => decodeSort(initialSort));
  useEffect(() => onSortChange(encodeSort(sorting)), [sorting, onSortChange]);
  const [columnOrder, setColumnOrder] = useState<string[]>(loadOrder);
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>(loadHidden);
  useEffect(() => savePref(ORDER_PREF, columnOrder), [columnOrder]);
  useEffect(
    () =>
      savePref(
        HIDDEN_PREF,
        Object.keys(columnVisibility).filter((id) => columnVisibility[id] === false),
      ),
    [columnVisibility],
  );
  const table = useReactTable({
    data,
    columns,
    state: { sorting, columnOrder, columnVisibility },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getRowId: (r) => String(r.id),
  });
  const sortedRows = table.getRowModel().rows;
  const rows = useMemo(
    () =>
      hideDone
        ? sortedRows.filter((r) => r.original.status === "pending" || r.original.id === currentId)
        : sortedRows,
    [sortedRows, hideDone, currentId],
  );

  // ── Columns: drag a header to reorder, right-click to show / hide ─────────────────
  const headerRow = useRef<HTMLDivElement>(null);
  /** The header being dragged, and where the line shows it would land. */
  const [colDrag, setColDrag] = useState<{ id: string; lineX: number } | null>(null);

  const beginColumnDrag = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let dragging = false;
    let insert = 0;
    const cells = () =>
      Array.from(headerRow.current?.querySelectorAll<HTMLElement>("[data-col]") ?? []);

    const move = (ev: PointerEvent) => {
      if (!dragging) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return;
        dragging = true;
        document.documentElement.classList.add("dragging-track");
      }
      const cs = cells();
      const rects = cs.map((c) => c.getBoundingClientRect());
      insert = rects.filter((r) => ev.clientX > r.left + r.width / 2).length;
      const left = headerRow.current?.getBoundingClientRect().left ?? 0;
      // Half the 12 px grid gap before the column it lands in front of (or after the last).
      const x =
        insert < rects.length ? rects[insert].left - 6 : rects[rects.length - 1].right + 6;
      setColDrag({ id, lineX: x - left });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (!dragging) return;
      document.documentElement.classList.remove("dragging-track");
      swallowNextClick(); // not a sort
      setColDrag(null);
      const visible = table.getVisibleLeafColumns().map((c) => c.id);
      const from = visible.indexOf(id);
      const to = insert > from ? insert - 1 : insert;
      if (from < 0 || to === from) return;
      const next = visible.filter((c) => c !== id);
      next.splice(to, 0, id);
      // Hidden columns keep their place at the end; shown again, they come back there.
      setColumnOrder([...next, ...columnOrder.filter((c) => !next.includes(c))]);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const onHeaderMenu = () =>
    void showColumnsMenu(
      columnOrder.map((id) => ({
        id,
        label: columnLabel(id),
        visible: columnVisibility[id] !== false,
        locked: id === LOCKED_COLUMN,
      })),
      {
        onToggle: (id) =>
          setColumnVisibility((v) => ({ ...v, [id]: v[id] === false })),
        onReset: () => {
          removePref(ORDER_PREF);
          removePref(HIDDEN_PREF);
          setColumnOrder(COLUMN_IDS);
          setColumnVisibility({});
        },
      },
    );

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
      <div
        role="rowgroup"
        className="sticky top-0 z-10 border-b border-border bg-background"
        onContextMenu={(e) => {
          e.preventDefault();
          onHeaderMenu();
        }}
      >
        {table.getHeaderGroups().map((hg) => (
          <div
            key={hg.id}
            ref={headerRow}
            role="row"
            className="relative grid gap-3 px-3"
            style={{ gridTemplateColumns }}
          >
            {colDrag && (
              <span
                className="pointer-events-none absolute inset-y-1 w-0.5 rounded bg-brand"
                style={{ left: colDrag.lineX - 1 }}
              />
            )}
            {hg.headers.map((h) => {
              const sorted = h.column.getIsSorted();
              const numeric = h.column.columnDef.meta?.numeric;
              return (
                <button
                  key={h.id}
                  data-col={h.column.id}
                  role="columnheader"
                  title="Click to sort · drag to move · right-click for columns"
                  onPointerDown={(e) => beginColumnDrag(e, h.column.id)}
                  aria-sort={
                    sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : "none"
                  }
                  onClick={h.column.getToggleSortingHandler()}
                  className={cn(
                    "flex h-7 min-w-0 items-center gap-1 text-xs font-medium text-muted-foreground select-none hover:text-foreground",
                    numeric && "justify-end",
                    colDrag?.id === h.column.id && "text-brand opacity-60",
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

      {rows.length === 0 && hideDone && (
        <div className="flex flex-col items-center gap-2 py-10 text-sm text-muted-foreground">
          <p>All tracks done.</p>
          <button
            type="button"
            onClick={onShowDone}
            className="text-xs text-brand hover:underline"
          >
            Show done tracks
          </button>
        </div>
      )}

      <div role="rowgroup" className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((v) => {
          const row = rows[v.index];
          const current = row.original.id === currentId;
          return (
            <div
              key={row.id}
              role="row"
              aria-selected={current}
              onPointerDown={
                DECIDABLE_STATUS.includes(row.original.status)
                  ? (e) => beginTrackDrag(e, row.original, onDropTrack)
                  : undefined
              }
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
