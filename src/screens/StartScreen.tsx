import { useEffect, useState } from "react";
import { FolderOpen, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api, type RecentSession } from "@/lib/api";
import { fileName, parentPath, tildify } from "@/lib/format";
import { importFolder, openFolder, useApp } from "@/state/app";

/** Import a folder (button, ⌘O or drop anywhere) or continue a recent session. */
export function StartScreen() {
  const opening = useApp((s) => s.opening);
  const error = useApp((s) => s.error);
  const [recent, setRecent] = useState<RecentSession[]>([]);

  const loadRecent = () => api.recentSessions().then(setRecent, () => setRecent([]));
  useEffect(() => {
    void loadRecent();
  }, []);

  /** Forgets the session (asks first if it has decisions); the folder can be imported anew. */
  const forget = async (r: RecentSession) => {
    try {
      if (await api.forgetSession(r.id)) await loadRecent();
    } catch {
      await loadRecent();
    }
  };

  /** A folder moved in Finder: point the session at its new place, then continue it. */
  const locate = async (r: RecentSession) => {
    try {
      const session = await api.locateSession(r.id);
      if (session) await openFolder(session.sourcePath, "triage");
    } catch (e) {
      useApp.setState({ error: String(e) });
    }
    await loadRecent();
  };

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
              <li key={r.id} className="group flex items-center rounded-md hover:bg-accent">
                <button
                  disabled={opening}
                  onClick={() =>
                    void (r.available ? openFolder(r.sourcePath, "triage") : locate(r))
                  }
                  title={
                    r.available
                      ? r.sourcePath
                      : `${r.sourcePath} was not found. Click to locate the folder (decisions and undo history are kept).`
                  }
                  className="flex min-w-0 flex-1 items-center gap-3 py-2 pl-3 text-left disabled:pointer-events-none disabled:opacity-40"
                >
                  <span className={r.available ? "min-w-0 flex-1" : "min-w-0 flex-1 opacity-50"}>
                    <span className="block truncate text-sm font-medium">
                      {fileName(r.sourcePath)}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {tildify(parentPath(r.sourcePath))}
                    </span>
                  </span>
                  {r.available ? (
                    <span className="tabular shrink-0 text-xs text-muted-foreground">
                      {recentStatus(r)}
                    </span>
                  ) : (
                    <span className="shrink-0 text-xs text-warning">
                      Not found · <span className="underline underline-offset-2">Locate…</span>
                    </span>
                  )}
                </button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  disabled={opening}
                  onClick={() => void forget(r)}
                  title="Forget this session (the files stay where they are)"
                  aria-label={`Forget ${fileName(r.sourcePath)}`}
                  className="mx-1 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                >
                  <X />
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}

function recentStatus(r: RecentSession): string {
  if (r.total === 0) return "Not scanned";
  if (r.remaining === 0) return "Done";
  return `${r.remaining} of ${r.total} left`;
}
