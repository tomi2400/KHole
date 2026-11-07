import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";

type Check = { label: string; state: "pending" | "ok" | "error"; detail: string };

const initial: Check[] = [
  { label: "Backend", state: "pending", detail: "" },
  { label: "Database", state: "pending", detail: "" },
  { label: "Analyzer", state: "pending", detail: "" },
];

/** M1 placeholder: proves every layer is wired up. Replaced by the Start screen in M2. */
export default function App() {
  const [checks, setChecks] = useState<Check[]>(initial);

  const run = useCallback(async () => {
    setChecks(initial);
    const settle = async (i: number, fn: () => Promise<string>) => {
      let next: Pick<Check, "state" | "detail">;
      try {
        next = { state: "ok", detail: await fn() };
      } catch (e) {
        next = { state: "error", detail: String(e) };
      }
      setChecks((cs) => cs.map((c, j) => (j === i ? { ...c, ...next } : c)));
    };
    await Promise.all([
      settle(0, api.ping),
      settle(1, async () => {
        const info = await api.appInfo();
        return `v${info.version} · schema ${info.schemaVersion} · ${info.dbPath}`;
      }),
      settle(2, async () => {
        const a = await api.analyzerInfo();
        return `${a.name} ${a.version} · Essentia ${a.essentia}`;
      }),
    ]);
  }, []);

  useEffect(() => {
    void run();
  }, [run]);

  return (
    <main className="flex h-full flex-col items-center justify-center gap-8 p-8">
      <h1 className="text-4xl font-semibold tracking-tight">
        K<span className="text-brand">Hole</span>
      </h1>
      <ul className="w-full max-w-xl divide-y divide-border rounded-lg border border-border">
        {checks.map((c) => (
          <li key={c.label} className="flex items-baseline gap-3 px-4 py-3 text-sm">
            <span
              className={
                c.state === "ok"
                  ? "text-brand"
                  : c.state === "error"
                    ? "text-destructive"
                    : "text-muted-foreground"
              }
              aria-label={c.state}
            >
              ●
            </span>
            <span className="w-20 font-medium">{c.label}</span>
            <span className="tabular truncate font-mono text-xs text-muted-foreground" title={c.detail}>
              {c.state === "pending" ? "…" : c.detail}
            </span>
          </li>
        ))}
      </ul>
      <Button variant="outline" onClick={() => void run()}>
        Run checks again
      </Button>
    </main>
  );
}
