import { invoke, convertFileSrc } from "@tauri-apps/api/core";

const log = (s: string) => { document.getElementById("log")!.textContent += s + "\n"; invoke("report", { line: s }); };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function once(a: HTMLAudioElement, ok: string, ms = 8000): Promise<string> {
  return new Promise((res) => {
    const t = setTimeout(() => res("timeout"), ms);
    a.addEventListener(ok, () => { clearTimeout(t); res("ok"); }, { once: true });
    a.addEventListener("error", () => { clearTimeout(t); res("error " + (a.error?.code ?? "?") + " " + (a.error?.message ?? "")); }, { once: true });
  });
}

async function test(path: string) {
  const name = path.split("/").pop()!;
  const a = new Audio();
  a.preload = "auto";
  a.volume = 0.05;
  const t0 = performance.now();
  a.src = convertFileSrc(path);
  const meta = await once(a, "loadedmetadata");
  if (meta !== "ok") return log(`${name.padEnd(24)} LOAD ${meta}`);
  const loadMs = Math.round(performance.now() - t0);
  let play = "ok";
  try { await a.play(); } catch (e) { play = "play() rejected: " + e; }
  await wait(1200);
  const advanced = a.currentTime > 0.5;
  let seek = "-";
  if (a.duration > 100) {
    const s0 = performance.now();
    a.currentTime = 500;
    const r = await once(a, "seeked");
    const seekMs = Math.round(performance.now() - s0);
    await wait(800);
    seek = `${r} ${seekMs}ms -> t=${a.currentTime.toFixed(1)}`;
  }
  a.pause();
  log(`${name.padEnd(24)} meta ${loadMs}ms dur=${a.duration.toFixed(1)} play=${play} advancing=${advanced} seek=${seek}`);
}

(async () => {
  log("UA " + navigator.userAgent);
  const files: string[] = await invoke("fixtures");
  for (const f of files) await test(f);
  log("DONE");
  await wait(500);
  await invoke("done");
})();
