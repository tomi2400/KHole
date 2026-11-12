import { Fragment } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { KEYMAP, SECTIONS, keyCaps } from "@/state/keymap";

/** What the mouse does; not in the keymap, so listed here. */
const MOUSE: { what: string; label: string }[] = [
  { what: "Double-click", label: "Play the track" },
  { what: "Drag a track", label: "Drop it on a destination tile or the Trash" },
  { what: "Click again", label: "Edit title, artist or file name of the current track" },
  { what: "Right-click", label: "Play, Reveal in Finder, Open With" },
  { what: "Drag a header", label: "Reorder columns (right-click: show / hide)" },
];

function Caps({ caps }: { caps: string[] }) {
  return (
    <span className="flex justify-end gap-1">
      {caps.map((c) => (
        <kbd
          key={c}
          className="tabular inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-muted px-1.5 text-[11px] font-semibold"
        >
          {c}
        </kbd>
      ))}
    </span>
  );
}

/** The `?` overview: every shortcut, built from the keymap table. */
export function ShortcutsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100%-4rem)] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>One key per action, then the next track still to do.</DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-x-8 gap-y-4">
          {SECTIONS.map((section) => (
            <section key={section}>
              <h3 className="mb-1.5 text-xs font-medium text-muted-foreground">{section}</h3>
              <div className="grid grid-cols-[1fr_auto] items-center gap-x-3 gap-y-1.5">
                {KEYMAP.filter((b) => b.section === section && !b.hidden).map((b) => (
                  <Fragment key={`${b.action}-${b.arg ?? ""}`}>
                    <span className="text-[13px]">{b.label}</span>
                    <Caps caps={keyCaps(b)} />
                  </Fragment>
                ))}
              </div>
            </section>
          ))}
        </div>
        <section>
          <h3 className="mb-1.5 text-xs font-medium text-muted-foreground">Mouse</h3>
          <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[13px]">
            {MOUSE.map((m) => (
              <Fragment key={m.what}>
                <span className="text-muted-foreground">{m.what}</span>
                <span>{m.label}</span>
              </Fragment>
            ))}
          </div>
        </section>
      </DialogContent>
    </Dialog>
  );
}
