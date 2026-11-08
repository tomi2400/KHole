import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api, type RecentSession } from "@/lib/api";
import { fileName, parentPath, tildify } from "@/lib/format";
import { importFolder, openFolder, useApp } from "@/state/app";

/** Import a folder (button, ⌘O or drop anywhere) or continue a recent session. */
export function StartScreen() {
  const opening = useApp((s) => s.opening);
  const error = useApp((s) => s.error);
  const [recent, setRecent] = useState<RecentSession[]>([]);

  useEffect(() => {
    api.recentSessions().then(setRecent, () => setRecent([]));
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void importFolder();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <main className="flex h-full flex-col items-center justify-center gap-10 p-8">
      <div className="flex flex-col items-center gap-5">
        <h1 className="text-4xl font-semibold tracking-tight">
          K<span className="text-brand">Hole</span>
        </h1>
        <Button size="lg" onClick={() => void importFolder()} disabled={opening}>
          <FolderOpen data-icon="inline-start" />
          Import Folder
          <kbd className="ml-1 text-xs opacity-60">⌘O</kbd>
        </Button>
        <p className="text-sm text-muted-foreground">or drop a folder here</p>
        {error && <p className="max-w-md text-center text-sm text-destructive">{error}</p>}
      </div>

      {recent.length > 0 && (
        <section className="w-full max-w-md">
          <h2 className="mb-2 px-3 text-xs font-medium text-muted-foreground">Recent sessions</h2>
          <ul className="flex flex-col">
            {recent.map((r) => (
              <li key={r.id}>
                <button
                  disabled={!r.available || opening}
                  onClick={() => void openFolder(r.sourcePath, "triage")}
                  title={r.sourcePath}
                  className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-left hover:bg-accent disabled:pointer-events-none disabled:opacity-40"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">
                      {fileName(r.sourcePath)}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {tildify(parentPath(r.sourcePath))}
                    </span>
                  </span>
                  <span className="tabular shrink-0 text-xs text-muted-foreground">
                    {recentStatus(r)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}

function recentStatus(r: RecentSession): string {
  if (!r.available) return "Folder not found";
  if (r.total === 0) return "Not scanned";
  if (r.remaining === 0) return "Done";
  return `${r.remaining} of ${r.total} left`;
}
