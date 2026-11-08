import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, RefreshCw, SlidersHorizontal } from "lucide-react";
import { DestinationBar, type Flash, type Notice } from "@/components/DestinationBar";
import { NowPlaying } from "@/components/NowPlaying";
import { TrackTable } from "@/components/TrackTable";
import { Waveform } from "@/components/Waveform";
import { Button } from "@/components/ui/button";
import { api, type Decision, type EditField, type Track } from "@/lib/api";
import { fileName, stem, tildify } from "@/lib/format";
import { goHome, goTo } from "@/state/app";
import { SEEK_LONG, SEEK_SHORT, useKeymap } from "@/state/keymap";
import { showOpenWithMenu, showTrackMenu } from "@/lib/trackMenu";
import { followTrack, loadTrack, seekBy, stop, togglePlay, usePlayer } from "@/state/player";
import { decide, editTrack, rescan, setStartAt, undo, useSession } from "@/state/session";

/** How long a notice (undo note, …) stays in the bar; warnings stay longer. */
const NOTICE_MS = 4000;
const WARNING_MS = 8000;

const UNDO_VERB = {
  move: "move",
  copy: "copy",
  trash: "trash",
  keep: "keep",
  rename: "rename",
  tag: "edit",
} as const;

/**
 * The work view: waveform + player on top, the track list, the destination bar below.
 * One key decides a track, then the next one still to do starts.
 */
export function TriageScreen() {
  const session = useSession((s) => s.session)!;
  const tracks = useSession((s) => s.tracks);
  const destinations = useSession((s) => s.destinations);
  const progress = useSession((s) => s.progress);
  const summary = useSession((s) => s.summary);
  const error = useSession((s) => s.error);

  // ── Notices and tile feedback ───────────────────────────────────────────────────────
  const [notice, setNoticeState] = useState<Notice | null>(null);
  const noticeTimer = useRef<number | undefined>(undefined);
  const setNotice = useCallback((text: string | null, tone: Notice["tone"] = "info") => {
    window.clearTimeout(noticeTimer.current);
    setNoticeState(text ? { text, tone } : null);
    if (text) {
      noticeTimer.current = window.setTimeout(
        () => setNoticeState(null),
        tone === "warning" ? WARNING_MS : NOTICE_MS,
      );
    }
  }, []);
  useEffect(() => () => window.clearTimeout(noticeTimer.current), []);

  const [flash, setFlash] = useState<Flash | null>(null);
  const [lastUsed, setLastUsed] = useState<number | "trash" | null>(null);
  const pulse = (tile: number | "trash", warning = false) => {
    setFlash((f) => ({ tile, n: (f?.n ?? 0) + 1, warning }));
    setLastUsed(tile);
  };

  // ── Playback ────────────────────────────────────────────────────────────────────────
  const trackId = usePlayer((s) => s.trackId);
  const byId = useMemo(() => new Map((tracks ?? []).map((t) => [t.id, t])), [tracks]);
  const current = trackId == null ? null : (byId.get(trackId) ?? null);
  const order = useRef<number[]>([]);
  const onOrderChange = useCallback((ids: number[]) => {
    order.current = ids;
  }, []);

  /**
   * Makes `track` current. Keeps playing if something was playing, unless told otherwise.
   * `reload` loads it again even if it is already current (its file moved).
   */
  const select = useCallback(
    (track: Track, autoplay = usePlayer.getState().playing, reload = false) => {
      if (track.id === usePlayer.getState().trackId && !reload) {
        if (autoplay && !usePlayer.getState().playing) togglePlay();
        return;
      }
      loadTrack(track, { autoplay, startAt: session.startAt });
      // Remembered for resuming (also when coming back from Setup).
      useSession.setState((s) => ({
        error: null,
        session: s.session && { ...s.session, currentTrack: track.id },
      }));
      api.setCurrentTrack(session.id, track.id).catch(() => {});
    },
    [session.id, session.startAt],
  );

  const step = (dir: 1 | -1) => {
    const ids = order.current;
    const i = trackId == null ? -1 : ids.indexOf(trackId);
    const next = i < 0 ? ids[0] : ids[i + dir];
    const track = next == null ? undefined : byId.get(next);
    if (track) select(track);
  };

  /** The next track still to do after `fromId` in the visible order, wrapping around. */
  const nextPending = (fromId: number): Track | undefined => {
    const all = new Map((useSession.getState().tracks ?? []).map((t) => [t.id, t]));
    const ids = order.current;
    const i = ids.indexOf(fromId);
    const rotated = i < 0 ? ids : [...ids.slice(i + 1), ...ids.slice(0, i)];
    return rotated.map((id) => all.get(id)).find((t) => t?.status === "pending");
  };

  // ── Decisions ───────────────────────────────────────────────────────────────────────
  // One at a time: a key pressed while a decision runs is dropped, never queued onto the
  // wrong track.
  const busy = useRef(false);

  const act = async (decision: Decision, tile: number | "trash" | null) => {
    if (busy.current || !current) return;
    busy.current = true;
    try {
      const outcome = await decide(current.id, decision);
      const dest = destinations.find((d) => d.id === outcome.destinationId);
      const where = dest ? fileName(dest.path) : "the destination";
      const warning = outcome.alreadyThere != null || outcome.renamedTo != null;
      if (tile != null) pulse(tile, warning);
      if (outcome.alreadyThere != null) {
        setNotice(
          outcome.kind === "copy"
            ? `Already in ${where} – not copied again.`
            : `Already in ${where} – not moved again; this copy went to the Trash (⌘Z brings it back).`,
          "warning",
        );
      } else if (outcome.renamedTo != null) {
        setNotice(
          `${where} already has a different file with this name – saved as “${outcome.renamedTo}”.`,
          "warning",
        );
      } else {
        setNotice(null);
      }
      const next = nextPending(current.id);
      if (next) select(next, session.autoNext);
      else {
        stop();
        if (!warning) setNotice("All tracks done.");
      }
    } catch (e) {
      setNotice(String(e), "warning");
    } finally {
      busy.current = false;
    }
  };

  const sortTo = (destinationId: number) =>
    void act({ kind: "sort", destinationId }, destinationId);

  const sortByKey = (key?: number) => {
    const dest = destinations.find((d) => d.hotkey === key);
    if (dest) sortTo(dest.id);
    else if (destinations.length === 0) setNotice("No destinations yet – add them in Setup.");
    else setNotice(`No destination on key ${key}.`);
  };

  const undoLast = async () => {
    if (busy.current) return;
    busy.current = true;
    try {
      const undone = await undo();
      if (!undone) {
        setNotice("Nothing to undo.");
        return;
      }
      const t = undone.track;
      setNotice(undone.note ?? `Undid ${UNDO_VERB[undone.kind]}: ${t.title ?? stem(t.path)}`);
      if (undone.kind === "rename" || undone.kind === "tag") {
        // An edit: stay where we are; the track keeps playing from the same spot.
        followTrack(t, undone.kind === "tag");
      } else {
        select(t, usePlayer.getState().playing, true);
      }
    } catch (e) {
      setNotice(String(e), "warning");
    } finally {
      busy.current = false;
    }
  };

  // ── Finder ──────────────────────────────────────────────────────────────────────────
  const showError = useCallback((e: string) => setNotice(e, "warning"), [setNotice]);
  const reveal = () => {
    if (current) api.revealTrack(session.id, current.id).catch((e) => showError(String(e)));
  };

  // ── Edits in the list ───────────────────────────────────────────────────────────────
  const onEdit = (track: Track, field: EditField, value: string) => {
    editTrack(track.id, field, value)
      .then((t) => {
        // A rewritten tag moves the audio bytes inside the file: reload from the same spot.
        if (field !== "file") followTrack(t, true);
        setNotice(null);
      })
      .catch((e) => setNotice(String(e), "warning"));
  };

  useKeymap({
    playPause: () => (current ? togglePlay() : step(1)),
    seekBack: () => seekBy(-SEEK_SHORT),
    seekForward: () => seekBy(SEEK_SHORT),
    seekBackLong: () => seekBy(-SEEK_LONG),
    seekForwardLong: () => seekBy(SEEK_LONG),
    prevTrack: () => step(-1),
    nextTrack: () => step(1),
    keep: () => void act({ kind: "keep" }, null),
    trash: () => void act({ kind: "trash" }, "trash"),
    sort: sortByKey,
    undo: () => void undoLast(),
    reveal,
  });

  // Resume where the session was left, else the first track still to do. Loaded paused:
  // opening a folder should not start making noise.
  const resumed = useRef(false);
  useEffect(() => {
    if (resumed.current || !tracks || tracks.length === 0) return;
    resumed.current = true;
    if (usePlayer.getState().trackId != null) return;
    const start =
      (session.currentTrack != null && tracks.find((t) => t.id === session.currentTrack)) ||
      tracks.find((t) => t.status === "pending") ||
      tracks[0];
    select(start, false);
  }, [tracks, session.currentTrack, select]);

  // What a rescan found changed in Finder. Files put back by hand (Trash → Put Back,
  // dragged back) are tracks to do again; renamed ones keep their state.
  useEffect(() => {
    if (!summary) return;
    const tracksN = (n: number) => `${n} ${n === 1 ? "track" : "tracks"}`;
    if (summary.returned > 0) {
      setNotice(`${tracksN(summary.returned)} came back into the folder – to do again.`);
    } else if (summary.missing > 0) {
      setNotice(
        `The file of ${tracksN(summary.missing)} disappeared from the folder (moved or deleted in Finder).`,
        "warning",
      );
    }
  }, [summary, setNotice]);

  // The current track was renamed, moved or deleted in Finder: follow it.
  useEffect(() => {
    if (current) followTrack(current);
  }, [current]);

  // Leaving Triage (Setup, Start) stops playback.
  useEffect(() => stop, []);

  const scanning = progress !== null;
  const total = tracks?.length ?? 0;
  const done = tracks?.filter((t) => t.status !== "pending" && t.status !== "missing").length ?? 0;
  const trashed = tracks?.filter((t) => t.status === "trashed").length ?? 0;
  const pct = scanning
    ? progress.total > 0
      ? (progress.done / progress.total) * 100
      : 0
    : total > 0
      ? (done / total) * 100
      : 0;

  return (
    <main className="flex h-full flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-3">
        <Button variant="ghost" size="icon-sm" onClick={goHome} title="Back to start">
          <ChevronLeft />
        </Button>
        <div className="min-w-0 flex-1" title={session.sourcePath}>
          <div className="truncate text-sm font-semibold">{fileName(session.sourcePath)}</div>
          <div className="truncate text-xs text-muted-foreground">
            {tildify(session.sourcePath)}
          </div>
        </div>
        <span className="tabular shrink-0 text-xs text-muted-foreground">
          {scanning
            ? progress.total > 0
              ? `Scanning ${progress.done} / ${progress.total}`
              : "Scanning…"
            : tracks
              ? `${done} / ${total} done${summary && summary.unreadable > 0 ? ` · ${summary.unreadable} unreadable` : ""}`
              : ""}
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => void rescan()}
          disabled={scanning}
          title="Rescan folder"
        >
          <RefreshCw className={scanning ? "animate-spin" : undefined} />
        </Button>
        <Button variant="ghost" size="sm" onClick={() => goTo("setup")} title="Session setup">
          <SlidersHorizontal data-icon="inline-start" />
          Setup
        </Button>
      </header>

      <div className="h-0.5 shrink-0">
        <div
          className={scanning ? "h-full bg-brand" : "h-full bg-brand/50"}
          style={{ width: `${pct}%`, transition: "width 150ms" }}
        />
      </div>

      <section className="shrink-0">
        <Waveform track={current} />
        <NowPlaying
          track={current}
          startAt={session.startAt}
          onStartAtChange={(v) => void setStartAt(v)}
          onReveal={reveal}
          onOpenWith={(anchor) => current && void showOpenWithMenu(session.id, current, anchor, showError)}
        />
      </section>

      {error && <p className="px-4 py-2 text-sm text-destructive">{error}</p>}

      {tracks && tracks.length > 0 ? (
        <TrackTable
          tracks={tracks}
          currentId={trackId}
          onSelect={(t) => select(t)}
          onActivate={(t) => select(t, true)}
          onOrderChange={onOrderChange}
          onEdit={onEdit}
          onContextMenu={(t) =>
            void showTrackMenu(session.id, t, { onPlay: () => select(t, true), onError: showError })
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-1 text-sm text-muted-foreground">
          {tracks && !scanning && (
            <>
              <p>No audio files in this folder.</p>
              {!session.includeSub && <p>Turn on Include subfolders in Setup to look deeper.</p>}
            </>
          )}
        </div>
      )}

      <DestinationBar
        destinations={destinations}
        trashed={trashed}
        mode={session.mode}
        flash={flash}
        lastUsed={lastUsed}
        notice={notice}
        onSort={sortTo}
        onTrash={() => void act({ kind: "trash" }, "trash")}
      />
    </main>
  );
}
