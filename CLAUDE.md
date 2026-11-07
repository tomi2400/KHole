# KHole

Macre készülő, nyílt forráskódú (GPL-3) DJ-triage app: Tauri 2 + React/TS + shadcn/ui, Rust backend, Essentia sidecar a BPM/key elemzéshez.

## Munkamenet eleje és vége
- **Először olvasd el:** `docs/DEVELOPMENT.md` – állapot, következő lépés, döntésnapló, nyitott kérdések.
- A termék forrása: `KHole – Product Spec.md`. Mérések: `docs/spike-results.md`.
- **A munkamenet végén frissítsd a `docs/DEVELOPMENT.md`-t:** Állapot tábla, a kész checkboxok, új döntések a Döntésnaplóba, egy új bejegyzés a Munkamenetek alá.

## Konvenciók
- A felhasználóval és a dokumentációban magyarul; a UI, a kód, a kommentek és a commit-üzenetek angolul.
- Fájlbiztonság (nem alku tárgya): soha nincs végleges törlés (Kuka), nincs felülírás (ütközésnél `name (2).ext`), minden művelet az undo naplóba kerül, csak a forrás- és a célmappákhoz nyúlunk.
- A fájlrendszerhez csak a Rust nyúl; a UI Tauri commandokat hív.
- Kukába dobás: `objc2-foundation` `NSFileManager.trashItemAtURL:resultingItemURL:`, nem a `trash` crate.
- A sidecar (`sidecar/khole-analyze`) stdinen mono f32le PCM-et kap, a dekódolás a Rust (symphonia) dolga.
- A `spike/` kód eldobható; a nagy fixture-fájlok nem mehetnek gitbe.
