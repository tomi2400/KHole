import { Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Track } from "@/lib/api";
import { formatBpm, formatDuration, stem } from "@/lib/format";
import { START_AT_OPTIONS, Segmented } from "@/components/Segmented";
import { togglePlay, usePlayer } from "@/state/player";

interface Props {
  track: Track | null;
  startAt: number;
  onStartAtChange: (value: number) => void;
}

/** The row under the waveform: play button, what is loaded, clock, start position. */
export function NowPlaying({ track, startAt, onStartAtChange }: Props) {
  const playing = usePlayer((s) => s.playing);
  const time = usePlayer((s) => s.time);
  const duration = usePlayer((s) => s.duration);
  const error = usePlayer((s) => s.error);

  const facts = track
    ? [
        track.bpm != null && `${formatBpm(track.bpm)} BPM`,
        track.musicalKey,
        formatDuration(track.duration),
        track.format,
        track.bitrate != null && `${track.bitrate} kbps`,
      ].filter(Boolean)
    : [];

  return (
    <div className="flex h-11 shrink-0 items-center gap-3 border-b border-border px-3">
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={togglePlay}
        disabled={!track || !!error}
        title={playing ? "Pause (Space)" : "Play (Space)"}
      >
        {playing ? <Pause /> : <Play />}
      </Button>

      <div className="flex min-w-0 flex-1 items-baseline gap-2">
        {track ? (
          <>
            <span className="truncate text-sm font-semibold" title={track.path}>
              {track.title ?? stem(track.path)}
            </span>
            {track.artist && (
              <span className="truncate text-sm text-muted-foreground">{track.artist}</span>
            )}
            <span className="tabular shrink-0 text-xs text-muted-foreground">
              {error ?? facts.join(" · ")}
            </span>
          </>
        ) : (
          <span className="text-sm text-muted-foreground">Nothing loaded</span>
        )}
      </div>

      <span className="tabular shrink-0 text-xs text-muted-foreground">
        {track ? `${formatDuration(time)} / ${formatDuration(duration)}` : ""}
      </span>

      <div className="flex shrink-0 items-center gap-2 text-xs" title="Where a track starts playing">
        <span className="text-muted-foreground">Start at</span>
        <Segmented
          label="Start playback at"
          value={startAt}
          options={START_AT_OPTIONS}
          onChange={onStartAtChange}
        />
      </div>
    </div>
  );
}
