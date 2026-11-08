import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/** Window-wide folder drop target. Shows an outline while something is dragged over. */
export function DropOverlay({ onDrop }: { onDrop: (path: string) => void }) {
  const [over, setOver] = useState(false);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setOver(true);
      else if (p.type === "leave") setOver(false);
      else if (p.type === "drop") {
        setOver(false);
        if (p.paths.length > 0) onDrop(p.paths[0]);
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [onDrop]);

  if (!over) return null;
  return (
    <div className="pointer-events-none fixed inset-2 z-50 flex items-center justify-center rounded-xl border-2 border-dashed border-brand bg-brand/10">
      <span className="rounded-md bg-background/90 px-3 py-1.5 text-sm font-medium">
        Drop to open this folder
      </span>
    </div>
  );
}
