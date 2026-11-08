import { useEffect, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import HoverPlugin from "wavesurfer.js/plugins/hover";
import TimelinePlugin from "wavesurfer.js/plugins/timeline";
import { api, CANCELLED, type Track } from "@/lib/api";
import { formatDuration } from "@/lib/format";
import { audio } from "@/state/player";

const HEIGHT = 128;
/** Bars in CSS px. Each bar covers ~1 s of a typical track at full window width. */
const BAR = 3;
const GAP = 2;
/** Share of the height above the baseline; the rest is the dim reflection. */
const TOP = 0.72;
const REFLECTION_ALPHA = 0.3;
const TIMELINE_HEIGHT = 16;
/** A flat line while the peaks of a new track are on their way. */
const FLAT = [new Float32Array([0])];

/**
 * SoundCloud-like bars: loudness (RMS) of each bar's time span, scaled to the loudest bar
 * of the track, with a dim reflection under the baseline. wavesurfer sets `fillStyle`
 * before calling this and masks the result for the played part, so only shapes are drawn.
 */
function renderBars(channels: Array<Float32Array | number[]>, ctx: CanvasRenderingContext2D) {
  const data = channels[0];
  const { width, height } = ctx.canvas;
  const dpr = window.devicePixelRatio || 1;
  const bar = Math.round(BAR * dpr);
  const step = bar + Math.round(GAP * dpr);
  const count = Math.floor(width / step);
  if (!data?.length || count === 0) return;

  const values = new Float32Array(count);
  let max = 0;
  for (let i = 0; i < count; i++) {
    const from = Math.floor((i * data.length) / count);
    const to = Math.max(from + 1, Math.floor(((i + 1) * data.length) / count));
    let sum = 0;
    for (let j = from; j < to; j++) sum += data[j] * data[j];
    values[i] = Math.sqrt(sum / (to - from));
    max = Math.max(max, values[i]);
  }

  const base = Math.round(height * TOP);
  const gap = Math.max(1, Math.round(dpr));
  const minH = Math.max(1, Math.round(dpr));
  ctx.beginPath();
  for (let i = 0; i < count; i++) {
    const h = Math.max(minH, max > 0 ? (values[i] / max) * base : 0);
    ctx.rect(i * step, base - h, bar, h);
  }
  ctx.fill();
  ctx.globalAlpha = REFLECTION_ALPHA;
  ctx.beginPath();
  for (let i = 0; i < count; i++) {
    const h = Math.max(minH, max > 0 ? (values[i] / max) * (height - base - gap) : 0);
    ctx.rect(i * step, base + gap, bar, h);
  }
  ctx.fill();
  ctx.globalAlpha = 1;
}

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Canvas needs real colors, not `var(--x)`; re-read when the system appearance flips. */
function colors() {
  return {
    waveColor: token("--waveform"),
    progressColor: token("--waveform-played"),
    cursorColor: token("--brand"),
  };
}

/**
 * Draws peaks without touching the audio element. The player sets `audio.src` first, so
 * sound never waits for the waveform. wavesurfer only swaps the source when the URL differs
 * from `currentSrc || src`, which is evaluated synchronously inside `load` (no fetch when
 * peaks are given), so passing that same value is always a no-op for playback.
 */
function show(ws: WaveSurfer, peaks: Float32Array[], duration: number): void {
  void ws.load(audio.currentSrc || audio.src, peaks, duration).catch(() => {});
}

/**
 * Full-width waveform of the loaded track: played part in another shade, purple playhead,
 * click or drag to seek. Draws peaks computed in Rust; it never decodes audio itself.
 */
export function Waveform({ track }: { track: Track | null }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WaveSurfer | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    const ws = WaveSurfer.create({
      container: containerRef.current!,
      media: audio,
      // Never let wavesurfer fetch and decode the file itself (it would, given only a URL).
      peaks: FLAT,
      duration: 1,
      height: HEIGHT,
      renderFunction: renderBars,
      cursorWidth: 2,
      dragToSeek: true,
      hideScrollbar: true,
      ...colors(),
      plugins: [
        TimelinePlugin.create({
          height: TIMELINE_HEIGHT,
          insertPosition: "afterend",
          formatTimeCallback: (s) => formatDuration(s),
          style: { fontSize: "10px", color: token("--muted-foreground") },
        }),
        HoverPlugin.create({
          lineColor: token("--muted-foreground"),
          lineWidth: 1,
          labelSize: 10,
          labelColor: token("--foreground"),
          labelBackground: token("--popover"),
          formatTimeCallback: (s) => formatDuration(s),
        }),
      ],
    });
    wsRef.current = ws;

    const media = window.matchMedia("(prefers-color-scheme: light)");
    const recolor = () => requestAnimationFrame(() => ws.setOptions(colors()));
    media.addEventListener("change", recolor);
    return () => {
      media.removeEventListener("change", recolor);
      ws.destroy();
      wsRef.current = null;
    };
  }, []);

  useEffect(() => {
    const ws = wsRef.current;
    if (!ws || !track) return;
    let current = true;
    setFailed(null);
    show(ws, FLAT, track.duration ?? 1);
    api.trackPeaks(track.id).then(
      (p) => {
        if (current) show(ws, [p.rms], p.duration);
      },
      (e) => {
        if (current && String(e) !== CANCELLED) setFailed(String(e));
      },
    );
    return () => {
      current = false;
    };
  }, [track?.id, track?.path]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="relative px-3 pt-3">
      <div ref={containerRef} className={track ? undefined : "invisible"} />
      {!track && (
        <p className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
          Select a track to listen.
        </p>
      )}
      {failed && (
        <p
          className="absolute inset-x-3 top-3 flex items-center justify-center text-xs text-muted-foreground"
          style={{ height: HEIGHT }}
          title={failed}
        >
          No waveform for this file.
        </p>
      )}
    </div>
  );
}
