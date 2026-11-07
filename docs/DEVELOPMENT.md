# KHole – Fejlesztési napló

Ez a fájl a fejlesztés egyetlen állapot-forrása: mi kész, mi következik, mit döntöttünk és miért.
**Minden munkamenet végén frissíteni kell** (Állapot, Következő lépés, Munkamenetek).

- Termékleírás: [KHole – Product Spec.md](../KHole%20–%20Product%20Spec.md)
- Spike-mérések részletesen: [spike-results.md](spike-results.md)
- Repo: https://github.com/tomi2400/KHole (publikus, `main` ág; az Essentia a `vendor/essentia` submodule-ban)

---

## Állapot

| Fázis | Tartalom | Állapot |
|---|---|---|
| 0. Spike | Lejátszás, waveform, Essentia sidecar, Kuka + undo | ✅ Kész (2026-11-07) |
| M1 Váz | git, Tauri + React + shadcn, CI, SQLite séma | 🟡 Kód kész (2026-11-07); hátravan: a CI első zöld futása, ablak szemrevételezése |
| M2 Import | Start képernyő, scan, tagek, lista | ⏭️ **Következő** |
| M3 Lejátszás | Waveform, billentyűk, kezdőpont | ⬜ |
| M4 Döntések | Setup képernyő, célmappák, move/copy/trash, auto-next | ⬜ |
| M5 Biztonság | Undo napló, fájlfigyelés, Reveal in Finder | ⬜ |
| M6 Tempó | Destination bar, Move to…, `?` lap, szűrő, oszlopok | ⬜ |
| M7 Elemzés | Essentia sidecar a háttérben, progress, cache | ⬜ |
| M8 Session | Mentés és folytatás, mappánkénti beállítások | ⬜ |
| v1.1+ | Spec #13–#22 | ⬜ |

---

## M1 – Váz (kód kész, 2026-11-07)

Cél: üres, de minden rétegében működő app, amelyre az M2-től építünk.

- [x] `git init`, `main` ág, remote: `https://github.com/tomi2400/KHole.git`
- [x] **Essentia submodule** `vendor/essentia` alatt, `23ae7ba`-n rögzítve (a régi `Essentia source/` klónt helyeztük át, nem töltöttük le újra). A `build.sh` alapból ezt használja.
- [x] `.gitignore` (node_modules, dist, target, `src-tauri/binaries/`, `.essentia-static/`, `spike/fixtures/` hangfájlok, `.DS_Store`)
- [x] `spike/` a kódjával gitbe megy referenciának, a fixture-hangfájlok nem (mind gitignore-ban)
- [x] GPL-3.0 `LICENSE`, angol `README.md`
- [x] Tauri 2 + React 19 + TS 6 + Vite 8 a gyökérben (a `src-tauri/binaries/` megmaradt)
- [x] Tailwind 4 + shadcn/ui (radix, Nova preset); sötét alapból, követi a rendszert (`.dark` osztály, `src/lib/theme.ts`); hangsúly `#895CF4` = `--brand`/`--primary`; `-apple-system` betű; `.tabular` osztály a számokhoz
- [x] Rust modulváz: `library/`, `audio/`, `analysis/`, `fileops/`, `store/`, `commands.rs`, `error.rs`; commandok: `ping`, `app_info`, `analyzer_info`
- [x] Sidecar: `bundle.externalBin`, hívás Rustból a `tauri-plugin-shell`-lel (`khole-analyze --version`); a sidecar új `--version` kapcsolót kapott (`analyzer_version` = 0.1.0)
- [x] Asset protokoll bekapcsolva, üres statikus scope-pal; a futásidejű megnyitás (`asset_protocol_scope().allow_directory`) az M2-ben a mappaválasztással jön
- [x] SQLite (`rusqlite` 0.41, `bundled`), migrációk `PRAGMA user_version`-nel, `src-tauri/src/store/migrations/0001_initial.sql`; WAL, foreign key-ek; 4 store-teszt
- [x] CI: `.github/workflows/ci.yml` (macos-latest arm64): typecheck, frontend build, sidecar build (Essentia cache submodule-commit + build.sh hash szerint), sidecar smoke test, fmt, clippy `-D warnings`, test, `tauri build --bundles app` + artifact
- [x] Első commit és push
- [ ] **CI első futása zöld** – ellenőrizni (az Essentia első fordítása a runneren lassú lesz, utána cache)
- [ ] **Ablak szemrevételezése:** `npm run tauri dev` → sötét ablak, három zöld (lila) pötty: Backend, Database, Analyzer. (Nem volt képernyőfelvételi engedélyem; a DB létrejött a 6 táblával, a sidecar a dev és a release buildben is fut.)

Helyben ellenőrizve: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test` (6 teszt), `npm run typecheck`, `npm run build`, `tauri build --bundles app` (14 MB-os `.app`, benne a sidecar).

## Következő lépés: M2 – Import

- [ ] Start képernyő: forrásmappa választása (`tauri-plugin-dialog`), „almappákkal” kapcsoló
- [ ] Asset scope megnyitása futás közben a kiválasztott mappára
- [ ] Scan (walkdir) háttérszálon, `scan_progress` esemény; `file_key` (méret + xxh3 az első/utolsó 64 KB-ra)
- [ ] Tagek (lofty): title, artist, BPM, key, hossz, formátum, bitráta → `tracks` tábla
- [ ] Session létrehozása / betöltése mappánként
- [ ] Lista (TanStack Table) a tagekkel, monospace számokkal
- [ ] Tesztfixture-generáló script (a kicsi hangfájlok helyett gitben csak a script legyen)

### SQLite séma (első változat)

```sql
CREATE TABLE sessions (
  id            INTEGER PRIMARY KEY,
  source_path   TEXT NOT NULL UNIQUE,     -- mappánként egy session
  include_sub   INTEGER NOT NULL DEFAULT 0,
  mode          TEXT NOT NULL DEFAULT 'move',   -- 'move' | 'copy'
  start_at      REAL NOT NULL DEFAULT 0,        -- 0 / 0.25 / 0.40
  auto_next     INTEGER NOT NULL DEFAULT 1,
  current_track INTEGER,                         -- folytatáshoz
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE destinations (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  hotkey     INTEGER,            -- 1..9, 0 (=10.); NULL = csak a Move to… keresőből
  position   INTEGER NOT NULL,
  UNIQUE (session_id, path)
);

CREATE TABLE tracks (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,      -- jelenlegi hely
  file_key   TEXT NOT NULL,      -- tartalom-alapú kulcs (lásd lent)
  title TEXT, artist TEXT, bpm REAL, musical_key TEXT,
  duration REAL, format TEXT, bitrate INTEGER,
  bpm_source TEXT,               -- 'tag' | 'analysis'
  status     TEXT NOT NULL DEFAULT 'pending',  -- pending | kept | moved | copied | trashed | missing
  UNIQUE (session_id, path)
);

CREATE TABLE actions (           -- undo napló
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  track_id   INTEGER REFERENCES tracks(id),
  kind       TEXT NOT NULL,      -- move | copy | trash | keep
  src        TEXT NOT NULL,
  dst        TEXT,               -- célútvonal, ill. a Kukában lévő útvonal
  renamed    INTEGER NOT NULL DEFAULT 0,   -- ütközés miatt sorszámot kapott
  ts         INTEGER NOT NULL,
  undone     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE analysis_cache (    -- fájlhoz kötve, nem sessionhöz
  file_key   TEXT PRIMARY KEY,
  bpm REAL, bpm_essentia REAL, bpm_confidence REAL,
  musical_key TEXT, scale TEXT, key_strength REAL,
  analyzer_version TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE peaks_cache (
  file_key TEXT PRIMARY KEY,
  per_sec  INTEGER NOT NULL,
  duration REAL NOT NULL,
  data     BLOB NOT NULL         -- min/max párok, i8 vagy i16
);
```

`file_key` = méret + az első és utolsó 64 KB hash-e (például xxh3). **Az mtime nem kerül bele**, mert másolásnál megváltozhat, így a cache átmozgatás és másolás után is talál.

---

## Architektúra (cél)

```
src-tauri/ (Rust)
  library/   scan (walkdir), tags (lofty), watcher (notify)
  audio/     peaks (symphonia → waveform-csúcsok, cache)
  analysis/  sidecar futtatása, munkasor (N szál), cache
  fileops/   move/copy/trash (objc2 NSFileManager), ütközés → "name (2).ext", undo napló
  store/     SQLite (rusqlite) + migrációk
  commands.rs  Tauri commandok + események (scan_progress, track_updated, analysis_progress)
  binaries/  khole-analyze-aarch64-apple-darwin (build.sh állítja elő, nincs gitben)
sidecar/khole-analyze/  C++ CLI Essentiára → JSON
src/ (React + TS + shadcn/ui + Tailwind)
  screens/    Start, Setup, Triage, (Summary v1.1)
  components/ Waveform (wavesurfer.js, előre számolt peaks), TrackTable (TanStack Table),
              DestinationBar, MoveToDialog (shadcn Command), ProgressBar
  state/      zustand store, központi keymap / billentyű-dispatcher
```

Alapelvek:
- A fájlrendszerhez **csak a Rust** nyúl; a UI commandokat hív.
- Semmi nem vész el: törlés = Kuka, ütközésnél nincs felülírás, minden művelet naplózva és visszavonható.
- Csak a forrás- és a célmappákhoz nyúl (asset scope és fájlműveletek is).

---

## Döntésnapló

| Dátum | Döntés | Indok |
|---|---|---|
| 2026-11-07 | BPM/key: **Essentia sidecar** (C++ CLI), nem tiszta Rust | Pontosabb; a felhasználó választotta |
| 2026-11-07 | Előbb spike, aztán MVP | Kockázatok tisztázása a váz előtt |
| 2026-11-07 | Lejátszás: WebView `<audio>` + asset protokoll, **nincs Rust dekóder a lejátszáshoz** | Mind a 6 formátum szól, a seek gyors (spike) |
| 2026-11-07 | Waveform-csúcsok Rustban (symphonia 0.6), cache-elve, 100 csúcs/s | 10 perces 24-bit AIFF: 164 ms |
| 2026-11-07 | Tagek: lofty 0.25 | Minden formátum, < 1 ms/fájl |
| 2026-11-07 | Kuka: saját `objc2-foundation` hívás (`trashItemAtURL:resultingItemURL:`), **nem** a `trash` crate | A crate nem adja vissza a Kukában lévő útvonalat (undo-hoz kell), és Finder-engedélyt kér |
| 2026-11-07 | A sidecar nem dekódol: stdinen mono f32le PCM-et kap a Rusttól | Nem kell ffmpeg: kisebb app, kevesebb licencgond |
| 2026-11-07 | Essentia: statikus, `--lightweight=libsamplerate --fft=ACCELERATE` | Önálló, 7,4 MB-os bináris, csak rendszerkönyvtárakkal |
| 2026-11-07 | BPM = egyenes illesztése az Essentia ütésidőpontjaira (bővülő ablak) | Az Essentia nyers BPM-je 0,3–0,5-tel alacsony; illesztéssel 124,00 ± 0,01 |
| 2026-11-07 | Essentia forrás: **git submodule** (`vendor/essentia`, `23ae7ba`), csak a felső szint (a beágyazott test-submodule-ok nem kellenek) | Így a CI is le tudja fordítani a sidecart; a felhasználó döntése |
| 2026-11-07 | Bundle identifier: `io.github.tomi2400.khole` | Domain még nincs; később nem érdemes váltani, mert az `app_data_dir` (és a DB helye) ehhez kötődik |
| 2026-11-07 | A sidecart Rust hívja (`tauri-plugin-shell`), a UI nem kap shell-jogot | A UI csak commandokat hív; szűkebb capability |
| 2026-11-07 | DB-migrációk: beágyazott SQL fájlok + `PRAGMA user_version`, külső crate nélkül | Egyszerű, tranzakciónként egy migráció; kiadott migrációt nem módosítunk |
| 2026-11-07 | Tailwind 4 `source(".")` a `src/`-re szűkítve | Különben a `vendor/` és a `spike/` is pásztázódik (3,8 s → 0,1 s) |
| 2026-11-07 | Minimum macOS: 13.0 | Tauri 2 / WebKit; később szűkíthető |

---

## Nyitott kérdések

1. **BPM-pontosság valódi zenén** (tempóváltás, breakdown, half-time, technóra hangolt tartomány) → M7-ben valódi promómappán mérni.
2. **Elemzési sebesség:** teljes track vagy csak a középső 2 perc; hány szál → M7.
3. **Domain** (a specből). Az identifier ettől függetlenül `io.github.tomi2400.khole` marad.
4. **Aláírás és notarizáció** a terjesztéshez (Apple Developer ID) → a kiadás előtt.

---

## Fejlesztői környezet

- macOS 26.5.2, Apple Silicon (arm64), Xcode telepítve
- Rust 1.99.1, Node 24.18 / npm 11.16, gh 2.87
- Homebrew (2026-11-07): `eigen` 5.0.1, `libsamplerate` 0.2.2, `pkgconf`, `libyaml`; továbbá `fftw` + `libomp`, ezek már nem kellenek (Accelerate van helyettük)
- Sidecar build: `sidecar/khole-analyze/build.sh` (alapból a `vendor/essentia` submodule) → `src-tauri/binaries/khole-analyze-aarch64-apple-darwin`
- Az adatbázis helye: `~/Library/Application Support/io.github.tomi2400.khole/khole.db`

## Fájltérkép (jelenlegi)

| Útvonal | Mi ez |
|---|---|
| `KHole – Product Spec.md` | Termékleírás (magyar), a döntések forrása |
| `docs/DEVELOPMENT.md` | Ez a napló |
| `docs/spike-results.md` | Spike-mérések és -döntések |
| `src/` | React UI: `App.tsx` (M1 rendszerellenőrző képernyő), `lib/api.ts` (típusos command-hívások), `lib/theme.ts`, `components/ui/` (shadcn) |
| `src-tauri/src/` | Rust: `lib.rs` (setup, DB megnyitása), `commands.rs`, `error.rs`, modulok: `library/ audio/ analysis/ fileops/ store/` |
| `src-tauri/src/store/migrations/` | SQL migrációk (`0001_initial.sql`) |
| `src-tauri/binaries/` | A lefordított sidecar (build-termék, nincs gitben) |
| `sidecar/khole-analyze/` | `main.cpp` + `build.sh` – az elemző sidecar |
| `vendor/essentia/` | Essentia submodule (`23ae7ba`) |
| `.github/workflows/ci.yml` | CI |
| `spike/probe/` | Rust próba: csúcsok, tagek, Kuka, `--pcm` dekódoló (eldobható; a kódrészletek átvehetők) |
| `spike/playback/` | Tauri lejátszási teszt (eldobható) |
| `spike/fixtures/` | Generált tesztfájlok (124 BPM, A-moll; nincsenek gitben) |

---

## Munkamenetek

### 2026-11-07 – Tervezés + spike
- Elkészült a fejlesztési terv (fázisok, mérföldkövek, architektúra).
- Spike: mind a 6 formátum szól a WebView-ban; waveform-csúcsok < 200 ms; lofty minden formátumot olvas; Kuka + visszaállítás objc2-vel működik.
- Homebrew-függőségek telepítve, Essentia beklónozva, sidecar lefordítva és tesztelve (BPM 124,00 ± 0,01, key A minor minden formátumon).
- A specben frissítve: AIFF = „Igen”, a nyitott kérdések közül kikerült.
- **Következő:** M1 (lásd fent).

### 2026-11-07 – M1 váz
- Git-repó, `main` ág, Essentia submodule (`vendor/essentia`) a meglévő klónból; a `spike/probe` üres beágyazott `.git`-je a scratchpadba került.
- Tauri 2 + React + TS + Tailwind 4 + shadcn/ui váz a gyökérben; KHole-téma (sötét, `#895CF4`, SF Pro).
- Rust modulváz, `ping` / `app_info` / `analyzer_info` commandok, SQLite + első migráció (6 tábla), 6 unit teszt.
- A sidecar `--version` kapcsolót kapott; a `build.sh` alapból a submodule-ból fordít (helyben a nulláról újrafordult, működik).
- CI workflow, LICENSE (GPL-3.0), README; a release `.app` 14 MB, a sidecar benne van és fut.
- **Nyitva:** a CI első futásának eredménye; az ablak szemrevételezése (nem volt képernyőfelvételi engedély).
- **Következő:** M2 (lásd fent).
