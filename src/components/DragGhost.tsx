import { Music } from "lucide-react";
import { stem } from "@/lib/format";
import { useDrag } from "@/state/drag";

/** The label that follows the pointer while a track is dragged onto a tile. */
export function DragGhost() {
  const track = useDrag((s) => s.track);
  const x = useDrag((s) => s.x);
  const y = useDrag((s) => s.y);
  if (!track) return null;
  return (
    <div
      className="pointer-events-none fixed z-50 flex max-w-64 items-center gap-1.5 rounded-md border border-brand bg-popover px-2 py-1 text-[13px] shadow-lg"
      style={{ left: x + 12, top: y + 8 }}
    >
      <Music className="size-3.5 shrink-0 text-brand" />
      <span className="truncate">{track.title ?? stem(track.path)}</span>
    </div>
  );
}
