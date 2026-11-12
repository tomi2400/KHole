import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import type { Destination, Track } from "@/lib/api";
import { fileName, parentPath, stem, tildify } from "@/lib/format";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The track to sort (the current one, or the one dropped on the Move to… tile). */
  track: Track | null;
  destinations: Destination[];
  mode: "move" | "copy";
  onPick: (destinationId: number) => void;
}

/**
 * Ranks a destination for the typed text: the folder name starting with it first (the
 * spec: type the start of the name, Enter), then a word of the name, then anywhere in the
 * name, then anywhere in the path.
 */
function score(value: string, search: string, keywords?: string[]): number {
  const q = search.trim().toLowerCase();
  if (!q) return 1;
  const [name = "", path = ""] = (keywords ?? []).map((k) => k.toLowerCase());
  if (name.startsWith(q)) return 1;
  if (name.split(/[\s_\-.]+/).some((w) => w.startsWith(q))) return 0.8;
  if (name.includes(q)) return 0.5;
  if (path.includes(q)) return 0.2;
  return value === q ? 0.1 : 0;
}

/** Move to… (M): finds any destination by name, also the ones without a number key. */
export function MoveToDialog({ open, onOpenChange, track, destinations, mode, onPick }: Props) {
  const title = track ? (track.title ?? stem(track.path)) : "";
  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Move to…"
      description="Type the start of a destination folder's name, then press Enter."
    >
      <Command filter={score}>
        <div className="truncate px-3 pt-2.5 pb-1.5 text-xs text-muted-foreground" title={title}>
          {mode === "copy" ? "Copy" : "Move"} <span className="text-foreground">{title}</span> to…
        </div>
        <CommandInput placeholder="Destination folder" />
        <CommandList>
          <CommandEmpty>No destination with that name.</CommandEmpty>
          {destinations.map((d) => (
            <CommandItem
              key={d.id}
              value={String(d.id)}
              keywords={[fileName(d.path), d.path]}
              onSelect={() => onPick(d.id)}
            >
              <kbd className="tabular flex size-5 shrink-0 items-center justify-center rounded bg-muted text-xs font-semibold">
                {d.hotkey ?? "·"}
              </kbd>
              <div className="min-w-0 flex-1" title={d.path}>
                <div className="truncate">{fileName(d.path)}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {tildify(parentPath(d.path))}
                </div>
              </div>
              <span className="tabular shrink-0 text-xs text-muted-foreground">
                {d.count || ""}
              </span>
            </CommandItem>
          ))}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
