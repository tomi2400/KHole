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
| M1 Váz | git, Tauri + React + shadcn, CI, SQLite séma | ✅ Kész (2026-11-07) |
| M2 Import | Start képernyő, scan, tagek, lista | ✅ Kész (2026-11-07) |
| M3 Lejátszás | Waveform, billentyűk, kezdőpont | ✅ Kész (2026-11-08) |
| M4 Döntések | Setup képernyő, célmappák, move/copy/trash, auto-next, ⌘Z | ✅ Kész (2026-11-08) |
| M5 Biztonság | Fájlfigyelés, Reveal in Finder, Open in… (az undo napló és a ⌘Z az M4-ben elkészült) | ✅ Kész (2026-11-08) |
| M5b Szerkesztés | Title / Artist / fájlnév helyben a listában, ⌘Z | ✅ Kész (2026-11-08) |
| M6 Tempó | Destination bar, Move to…, `?` lap, szűrő, oszlopok | ⬜ |
| M7 Elemzés | Essentia sidecar a háttérben, progress, cache | ⬜ |
| M8 Session | Mentés és folytatás, mappánkénti beállítások | ⬜ |
| v1.1+ | Spec #13–#22 | ⬜ |

---

## M1 – Váz (kész, 2026-11-07)

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
- [x] **CI első futása zöld** (run 42391106439, minden lépés sikeres; az Essentia azóta cache-ben)
- [x] **Ablak szemrevételezése:** `npm run tauri dev` → sötét ablak, három lila pötty: Backend, Database, Analyzer (a felhasználó ellenőrizte, 2026-11-07).

Helyben ellenőrizve: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test` (6 teszt), `npm run typecheck`, `npm run build`, `tauri build --bundles app` (14 MB-os `.app`, benne a sidecar).

## M2 – Import (kész, 2026-11-07)

- [x] Start képernyő: **Import Folder** gomb (natív mappaválasztó Rustból, `tauri-plugin-dialog`), `⌘O`, „or drop a folder here”, **Recent sessions** (utolsó 5, „12 of 40 left” / „Done” / „Folder not found”)
- [x] Drag & drop az egész ablakra (`onDragDropEvent`, lila keret); egy ráejtett trackfájl a mappáját nyitja meg
- [x] Asset scope megnyitása futás közben a kiválasztott mappára (`allow_directory`, rekurzívan)
- [x] Scan (walkdir) háttérszálon (`spawn_blocking`), `scan_progress` esemény 50 ms-onként; a rejtett fájlok (`.x`, `._x`) és a symlinkek kimaradnak; egyszerre egy scan sessiononként
- [x] `file_key` = méret (hex) + xxh3 az első/utolsó 64 KB-ra (`library/file_key.rs`)
- [x] Tagek (lofty): title, artist, BPM (`TBPM`/`tmpo`/`BPM`, tizedessel is, 40–300 közt), key (`TKEY`/`INITIALKEY`), hossz, formátum (MP3/WAV/AIFF/FLAC/AAC/ALAC), bitráta → `tracks` tábla, 64 soros tranzakciókban
- [x] Session létrehozása / betöltése mappánként (`store/sessions.rs`); **Include subfolders** kapcsoló, váltáskor újrascan
- [x] Újrascan: új fájl be, ismert fájl tagjei frissülnek, a státusz megmarad; eltűnt fájl sora törlődik, ha az undo napló nem hivatkozik rá
- [x] Lista (TanStack Table 8 + react-virtual): #, Title (tag nélkül a fájlnév), Artist, BPM, Key, Time, Format, kbps, Status; fejlécre kattintva rendezhető; 28 px sorok, tabular számok; kezelt trackek halványítva
- [x] Tesztfixture-generáló: `cargo run --example make_fixtures -- <üres mappa>` (12 track, 5 formátum, részben hiányzó tagekkel, almappákkal, rejtett és nem audio fájllal; MP3 nincs, mert a macOS-nek nincs MP3-enkódere)
- [x] **Kézi teszt az ablakban** (a felhasználó elvégezte, rendben, 2026-11-07)

Mérés: a 12 generált fájl scanje + tagolvasása 11 ms (debug). Mind a 6 spike-fixture formátuma helyes, az ALAC is.
Tesztek: 19 unit teszt + 2 `#[ignore]` (spike-fixturek; `KHOLE_SCAN_DIR=… cargo test scan_dir_from_env -- --ignored --nocapture` tetszőleges mappára).

### Kézi teszt (M2)
1. `cd src-tauri && cargo run --example make_fixtures -- ~/Music/KHole-test`
2. `npm run tauri dev` → Start képernyő → Import Folder (vagy ráhúzás) → `KHole-test`
3. Elvárt: 10 track (almappák nélkül), a 06-os fájlnévvel, a 09-es csonkolva; Include subfolders be → 12; rendezés BPM-re, a hiányzók alul; vissza → Recent sessions-ben „12 of 12 left”.

## M3 – Lejátszás (kész, 2026-11-08)

- [x] Waveform-adat Rustban (`audio/peaks.rs`, symphonia 0.6): mono **RMS + csúcs** vödrönként (formátum 2; az 1-es min/max volt), 100 vödör/s, i8; a vödörhatár `⌊(k+1)·sr/100⌋`, így a nem osztható mintavételnél sem csúszik; csonkolt fájlnál a meglévő részt rajzolja
- [x] `peaks_cache` `file_key` szerint, a kulcsot a fájlból számoljuk (nem a sorból), így kicserélt fájlnál nincs régi waveform; LRU-ürítés 64 MB felett (`0002` migráció: `used_at`; `0003` kiüríti a régi min/max sorokat)
- [x] `track_peaks` command nyers bináris válasszal (`ipc::Response`, nincs JSON), csak a legújabb kérés fut végig (↓ nyomva tartásakor a többi `cancelled`)
- [x] Lejátszás: egyetlen `<audio>` (`state/player.ts`), asset protokoll; a hang nem vár a waveformra
- [x] Waveform: wavesurfer.js **7.12.12** a közös `<audio>`-ra kötve, előre számolt adat, saját `renderFunction`: 3 px-es oszlopok 2 px réssel, az oszlop idősávjának RMS-e a track leghangosabb oszlopára skálázva, alul 30%-os tükörkép (SoundCloud-szerű); lejátszott rész halványabb, lila lejátszófej, kattintás/húzás = ugrás, idővonal + hover-idő; színek a `--waveform` / `--waveform-played` tokenekből, rendszertéma-váltáskor újraszínez
- [x] Alatta sor: Play/Pause, cím, előadó, BPM · key · hossz · formátum · kbps, óra (`1:23 / 6:45`), **Start at 0% / 25% / 40%**
- [x] Billentyűk, központi keymap (`state/keymap.ts`, capture fázisban, szövegmezőben nem): Space, ←/→ ±15 mp, Shift ±60 mp, ↑/↓ trackváltás a **látható (rendezett) sorrendben**; nyomva tartva ismétel (Space nem)
- [x] Lista: aktuális track lila kiemeléssel, görget hozzá; kattintás = kiválasztás, dupla kattintás = lejátszás
- [x] Kezdőpont (`set_start_at`, 0–0,9 közt validálva), az aktuális track mentése (`set_current_track`, csak a session saját trackje); megnyitáskor onnan folytat, különben az első `pending`, **szünetelve**
- [x] **Kézi teszt az ablakban**, az új (RMS-) waveformmal együtt (a felhasználó elvégezte, rendben, 2026-11-08)

Mérés (`cargo test peaks_of_spike_fixtures -- --ignored --nocapture`): 10 perces 24-bit AIFF/WAV 0,20–0,22 s release, 0,30 s dev; 30 s-os fájlok 12–50 ms.
Tesztek: 28 unit teszt + 3 `#[ignore]`.

### Kézi teszt (M3)
1. `npm run tauri dev` → egy mappa (pl. `~/Music/KHole-test` vagy a `spike/fixtures`) → az első track betöltve, szünetel, waveform látszik.
2. Space → szól; ←/→ és Shift+←/→ ugrik; kattintás és húzás a waveformon ugrik; a lejátszott rész halványabb.
3. ↑/↓ → trackváltás (ha szólt, tovább szól), a lista görget; BPM-re rendezés után a ↓ a rendezett sorrendet követi.
4. Start at 25% → a következő track a negyedénél indul.
5. Vissza a Startra, újra megnyitva ugyanazon a tracken folytat.
6. Rendszertéma váltása → a waveform színe követi.

## M4 – Döntések (kész, 2026-11-08)

- [x] Képernyők: Start → **Setup** → **Triage** (a `SessionScreen` lett a `TriageScreen`). Importált / ráejtett mappa a Setupra visz, a Recent sessions egyből a Triage-ra (a beállítások mappánként megvannak). A Triage fejlécében **Setup** gomb.
- [x] A session állapota (session, trackek, célmappák, scan) közös zustand store-ban (`state/session.ts`), így a Setupon indult scan a Triage-ban folytatódik
- [x] Setup: Source (mappa, trackszám / scan, Include subfolders), Destinations (Add folder natív választóval, szám 1–0 automatikusan, átírható – foglalt számnál csere –, eltávolítás; üresen: „No destinations – cleanup mode…”), When sorting: Move / Copy, Start playback at, Auto-play next track; **Start** gomb (fókuszban, Enter indít)
- [x] `fileops/fs.rs`: áthelyezés `renamex_np(RENAME_EXCL)`-lel (atomi, nincs felülírás); ütközésnél `name (2).ext`; köteten át másolás + méretellenőrzés + az eredeti eltávolítása; másolás `O_EXCL` helyfoglalással; Kuka `NSFileManager.trashItemAtURL:resultingItemURL:` (objc2)
- [x] `fileops/mod.rs`: `decide` (keep / trash / sort = move vagy copy a session módja szerint) és `undo_last`; minden művelet az `actions` naplóba, a track sora követi a fájlt (áthelyezett / kukázott track útvonala = új hely)
- [x] Undo (⌘Z, több lépés mélyen): move/trash → vissza az eredeti helyre (foglalt névnél `(2)`, jelzéssel); copy → **a másolat a Kukába** (nem törlünk); keep → státusz vissza; ha a fájlt közben elvitték, a track `missing` lesz, a napló továbblép
- [x] Billentyűk: **Enter** = keep, **Backspace** = Kuka, **1–0** = célmappa, **⌘Z** = undo; egyszerre egy döntés fut, a közben jött gombnyomás elvész (nem a rossz trackre megy)
- [x] Auto-next: a következő `pending` track a látható (rendezett) sorrendben, körbefordulva; kikapcsolva betölti, de nem indítja; ha nincs több: „All tracks done.”
- [x] Egyszerű **Destination bar** alul: csempék számmal, névvel, számlálóval + Trash csempe; használatkor rövid lila villanás, az utoljára használt csempe lila keretet kap; kattintásra is dönt. Állapotsor: mód, gombok, értesítés (pl. „Name taken – saved as …”)
- [x] Fejléc: „132 / 480 done”, alatta vékony progress sáv
- [x] A célmappák kimaradnak a scanből (ha a forráson belül vannak), és a WebView streamelhet belőlük (asset scope)
- [x] Migrációk: `0004` (`actions.prev_status`, `actions.destination_id`), `0005` (index a track eredeti helye szerinti rendezéshez)
- [x] Első kézi teszt (a felhasználó, 2026-11-08): minden működött, két hibával (lent javítva)
- [x] **Javítás 1 – Kukából kézzel visszatett track:** eddig szürke maradt („missing”), újrascan után duplikálva jelent meg, és egyiket sem lehetett kezelni. Most: a scan egy tracké olvasztja össze, ha az eredeti sor fájlja eltűnt a rögzített helyéről, azonos a tartalomkulcs, és az új sor pontosan ott van, ahonnan egy akció elvitte (`tracks::reattach_returned`; a régi sor megkapja az új útvonalát, státuszát és akcióit, a saját nyitott akciói lezárulnak). A ⌘Z is felismeri, ha a fájl már a helyén van. Értesítés: „1 track came back into the folder – to do again.” A nálad már duplikált sort (3 + 101) a következő scan rendbe teszi.
- [x] **Javítás 2 – ugyanaz a track már a célmappában** (azonos név + tartalomkulcs): Copy módban nem másol újra, Move módban a forrásfájl a Kukába kerül (⌘Z visszahozza); a track Copied/Moved, a számláló nő. **Borostyánszínű figyelmeztető sáv** (8 mp) + kétszeri borostyán villanás a csempén: „Already in Keep – not copied again.” Ugyanez a sáv jelzi a más tartalmú, azonos nevű fájl miatti `(2)`-es átnevezést és a hibákat. (`0006` migráció: `actions.duplicate`, hogy a duplikált másolás visszavonása ne kukázza a már ott lévő fájlt.)
- [x] **A két javítás kipróbálása** (a felhasználó, rendben, 2026-11-08)

Tesztek: 54 unit teszt + 7 `#[ignore]` (köztük négy valódi Kukás teszt: `cargo test trash duplicate_move put_back -- --ignored`, helyben lefutottak).

### Kézi teszt (M4)
1. `cd src-tauri && cargo run --example make_fixtures -- ~/Music/KHole-test` (ha még nincs), és csinálj két üres célmappát, pl. `~/Music/KHole-keep`, `~/Music/KHole-maybe`.
2. `npm run tauri dev` → Import Folder → `KHole-test` → **Setup**: Add folder ×2 (1 és 2 számot kapnak); a 2-esnél állítsd a számot 1-re → a kettő cserél.
3. Start → Triage: **1** → a track átkerül, a csempe villan, a számláló 1, indul a következő. **Backspace** → Kukába (Finderben a Kukában látszik). **Enter** → Kept, a fájl marad.
4. **⌘Z** háromszor → mindhárom visszajön (a Kukából is), a számláló visszaáll.
5. Tegyél a célmappába egy ugyanolyan nevű fájlt, majd küldd oda a tracket → „Name taken – saved as … (2)…”, a meglévő fájl érintetlen.
6. Setup → Copy → **2** → a forrásban is megmarad, a track „Copied”; ⌘Z → a másolat a Kukába kerül.
7. Auto-play kikapcsolva → döntés után a következő betöltődik, de nem szól.
8. Vissza a Startra, majd Recent sessions → egyből a Triage, a célmappák és a számlálók megvannak.
9. Backspace egy trackre → a Finderben Kuka → Put Back → az appban Rescan (vagy újranyitás) → a track újra döntetlen, nincs duplikátum, dönthető.
10. Copy módban küldj egy tracket egy célmappába, ahol már ott van ugyanez a fájl → borostyán sáv „Already in … – not copied again.”, nincs `(2)`; Move módban ugyanez → a forrás a Kukába kerül, ⌘Z visszahozza.

## M5 – Biztonság (kész, 2026-11-08)

- [x] **Fájlfigyelés** (`library/watch.rs`, notify 8.2 / FSEvents): a session forrásmappáját figyeli (a megnyitott sessiont; másik session megnyitása lecseréli, a Startra lépés leállítja). Az események 400 ms csendig gyűlnek (hosszú másolásnál legfeljebb 2 mp-enként jelez), utána `folder_changed` esemény → a UI csendes újrascant futtat (nincs progress sáv; ha közben már fut egy scan, utána még egy jön). A szűrés a scan szabályait követi: rejtett fájlok, a forráson belüli célmappák, kikapcsolt Include subfoldersnél az almappák kimaradnak.
- [x] **A saját műveleteink nem indítanak scant:** a `decide` és az `undo_last` a `watch::own_op()` őrrel jelzi, hogy fut, és felírja az érintett útvonalakat (5 mp-ig figyelmen kívül); amíg az őr él, a figyelő visszatartja az eseményeket.
- [x] **Átnevezés / áthelyezés a forráson belül** (`tracks::relink_renamed`): ha egy track fájlja eltűnt, és ugyanazzal a tartalomkulccsal új fájl jelent meg, az ugyanaz a track marad (id, státusz, undo napló, aktuális track); a `missing` újra `pending` lesz.
- [x] **Eltűnt fájl** (`tracks::mark_missing`): a döntött (naplóban szereplő) track sora `missing` lesz, nem törlődik; a döntetlen track sora eltűnik, mint eddig. Ha a fájl visszajön, a track újra `pending`. Borostyán értesítés: „The file of 1 track disappeared from the folder…”.
- [x] A lejátszó követi a szóló tracket: ha a Finderben átnevezték / áthelyezték, ugyanonnan szól tovább az új helyről; ha törölték, megáll („The file is gone…”).
- [x] **Reveal in Finder** (⌘R, gomb a NowPlaying sorban, jobb klikk a listán): `reveal_track` → `tauri-plugin-opener` `reveal_item_in_dir` (Rustból).
- [x] **Open in…** (gomb + a jobb klikkes menü „Open With” almenüje): `track_apps` = `NSWorkspace.URLsForApplicationsToOpenURL:` (az alapértelmezett elöl, „(default)” jelöléssel, a duplikált appok egyszer); `open_track` = `/usr/bin/open -a <app> <fájl>`, de csak olyan appal, amelyet a macOS maga kínál a fájlhoz.
- [x] **Natív macOS menük** (`lib/trackMenu.ts`, `@tauri-apps/api/menu`): jobb klikk egy soron → Play / Reveal in Finder / Open With ▸. A menüpontok csak Rust commandokat hívnak.
- [x] **File oszlop** a listában (fájlnév kiterjesztés nélkül, a Title után, rendezhető): a Title a tagből jön, így az átnevezés eddig nem látszott (a felhasználó kézi tesztje alapján)
- [ ] Undo session → v1.1 a Summary-val (a napló megvan hozzá).

Tesztek: 62 unit teszt + 7 `#[ignore]`; új: relevancia-szűrő, saját útvonalak, valódi FSEvents-folyam, átnevezés (két track, köztük az aktuális), másolat ≠ átnevezés, eltűnt → `missing` → vissza, appok egy WAV-hoz, nem kínált app elutasítása.

### Kézi teszt (M5)
1. `npm run tauri dev`, nyiss meg egy sessiont (pl. `~/Music/KHole-test`), Triage.
2. Finderben **nevezz át** egy tracket a forrásban → kb. fél mp múlva a listában az új név; ha döntött volt (Kept), az is marad. Ha épp az szólt, szól tovább.
3. **Húzz be** egy új hangfájlt a mappába → megjelenik a listában. **Töröld** (Kukába) → eltűnik; ha előtte Keep-et kapott, „Missing” lesz és borostyán értesítés jön; Put Back → újra döntetlen.
4. Dönts gyorsan egymás után (1, Backspace, Enter, ⌘Z) → **nincs** újrascan-villogás (a saját műveletek ki vannak szűrve).
5. **⌘R** → Finder ablak, a track kijelölve. A Kukába dobott tracknél a Kukában mutatja.
6. **Open in…** gomb → natív menü az appokkal (a default elöl) → pl. Music / Audacity megnyitja.
7. **Jobb klikk** egy soron → Play / Reveal in Finder / Open With ▸.
8. Setupban kapcsold ki az Include subfolderst, majd tegyél fájlt egy almappába → nem jelenik meg (nincs scan sem).

## M5b – Helyben szerkesztés (kész, 2026-11-08)

A felhasználó kérése: a listában a Title, az Artist és a fájlnév legyen átírható (a BPM, a key és a többi nem). A spec eddig kizárta a metaadat-szerkesztést; módosítva (#23, MVP).

- [x] **Kezelés:** katt a kijelölt sor Title / Artist / File cellájára → 450 ms múlva szövegmező (a dupla katt továbbra is lejátszás, ez megszakítja). Enter vagy kikattintás = mentés, Esc = mégse. Üres Title / Artist = a tag törlése. Kukázott és eltűnt tracknél nem szerkeszthető.
- [x] **Átnevezés** (`fileops/edit.rs`): ugyanabban a mappában, a kiterjesztés marad; `rename_excl`, foglalt névnél hiba („… already exists”), nincs `(2)`; csak kis-/nagybetű váltásnál sima `rename`. Tiltott: üres név, ponttal kezdődő, `/` vagy `:`, 255 bájt felett.
- [x] **Tag írása** (`tags::write_text`): a formátum konkrét tagje (MP3/WAV/AIFF/AAC: ID3v2, FLAC: Vorbis, M4A: ilst), nem a lofty általános `Tag`-je, mert az eldobja az ismeretlen frame-eket (Serato GEOB). Ha nincs fő tag, a lista szerinti Title + Artist kerül az újba (pl. RIFF INFO-ból).
- [x] **Biztonság:** klón (`copyfile` CLONE + metaadatok, xattr) → írás a rejtett `.khole-edit-…` klónba → ellenőrzés (az új érték, a másik mező, BPM, key, formátum, hossz, képek száma) → atomi `rename` az eredetire. Hiba esetén az eredeti érintetlen, a klón törlődik (a saját ideiglenes fájlunk).
- [x] **Undo:** `rename` és `tag` akció a naplóban (`0007` migráció: az `actions` újraépítve a bővített CHECK miatt + `field`, `old_value`, `new_value`); ⌘Z visszanevez / visszaírja a régi értéket. A lejátszás nem ugrik vissza az elejére.
- [x] A lejátszó a tag írása után ugyanonnan tölti újra a fájlt (a bájtok elmozdulnak a fájlon belül); átnevezésnél követi az új útvonalat.
- [x] A sor `file_key`-je és tagjei frissülnek (a waveform-cache új kulcsot kap; az M7-es elemzési cache-t is érinti majd).

Tesztek: 70 unit teszt + 9 `#[ignore]`; új: Title írás + undo, Artist tag nélküli fájlon + törlés, változatlan érték = nincs akció, átnevezés + foglalt név + tiltott nevek + undo, kis-/nagybetű, átnevezett track újrascan után, nem szerkeszthető státusz, `0007` migráció meglévő naplóval. Helyben futtatva: mind a 6 formátum (`edit_spike_fixtures`), borító + Serato GEOB megmarad MP3-ban, kép FLAC-ban (`keeps_cover_art_and_serato_frames`). A valódi DB a futó `tauri dev`-ben a 7-es sémára állt, a 16 naplóbejegyzés megvan.

### Kézi teszt (M5b)
1. Triage: kattints egy sorra, majd még egyszer a Title cellájára → szövegmező. Írd át, Enter → a lista és a lejátszósor is az új címet mutatja; ha szólt, ugyanonnan szól tovább.
2. Ugyanez az Artistra; üresre törölve az Artist eltűnik.
3. File cella: írd át a nevet → a Finderben is átnevezve, a kiterjesztés marad. Egy már létező névre → borostyán hiba, semmi nem változik.
4. **⌘Z** egymás után → visszanevez, visszaírja a régi címet / előadót.
5. Dupla katt egy soron → lejátszik, nem nyit szerkesztést. Esc → mégse.
6. Egy Serato-val elemzett / cue pontos MP3-on: cím átírása után a Seratóban a cue pontok megvannak (ha van ilyen fájlod).

## Következő lépés: M6 – Tempó

- [ ] Destination bar húzható csempékkel (drag & drop a listából)
- [ ] Move to… (M): kereső a 10-en felüli célmappákhoz (shadcn Command)
- [ ] `?` lap: billentyűparancsok (a `KEYMAP` táblából)
- [ ] Kezelt trackek szűrője, átrendezhető oszlopok

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
| 2026-11-07 | A mappaválasztó dialógust Rust nyitja (`pick_folder` command), a UI nem kap `dialog:` jogot | Szűk capability; a UI csak commandokat hív |
| 2026-11-07 | DB-időbélyegek: Unix **ezredmásodperc** | A Recent sessions sorrendje egy másodpercen belül is egyértelmű |
| 2026-11-07 | Újrascan: az eltűnt fájl sora csak akkor törlődik, ha nincs rá hivatkozás az `actions` táblában | A mozgatott/kukázott trackek az undo miatt megmaradnak |
| 2026-11-07 | A track listában a tag-értékek vannak; az elemzés eredménye majd az `analysis_cache`-ből jön `file_key` szerint (M7) | Újrascan nem írja felül az elemzést |
| 2026-11-07 | ALAC vs. AAC: lofty csak a lossless MP4-hez ad bitmélységet | Külön MP4-parse nélkül, a fixture-ökön ellenőrizve |
| 2026-11-07 | TanStack Table **8.21** (nem 9) | A 9-es API-ja teljesen más; a 8 kiforrott |
| 2026-11-07 | wavesurfer.js **7.12.12**, nem a 8.x | A 8.0 szeptemberi, még sűrűn foltozzák; a 7-es API ismert és stabil |
| 2026-11-07 | A wavesurfer a közös `<audio>`-ra kötve, mindig peaks-szel töltve (a létrehozáskor is) | Csak URL-lel letöltené és WebAudióval dekódolná a teljes fájlt; ugyanazzal a `currentSrc`-vel hívva a `load` nem nyúl a lejátszáshoz |
| 2026-11-07 | A waveform **RMS**-t rajzol, nem csúcsot; tracken belül a leghangosabb részre skálázva | Masterelt zenénél a csúcs szinte végig teljes kivezérlés, tömör téglát adott (felhasználói visszajelzés); az RMS-en látszik a breakdown és a drop |
| 2026-11-07 | Peaks a UI-nak nyers bájtokként (`ipc::Response`) | Egy 10 perces track 120 KB a ~400 KB-os JSON helyett |
| 2026-11-07 | `peaks_cache` LRU, 64 MB plafon | Újraszámolni 0,2 s, nem éri meg korlátlanul tárolni |
| 2026-11-07 | Dev build: `opt-level = 1`, függőségek `3` | Debugban egy 10 perces waveform 2 s volt, így 0,3 s |
| 2026-11-07 | ↑/↓ megtartja a lejátszás állapotát; megnyitáskor szünetel | Mappanyitás ne szóljon váratlanul; lépkedés közben folyamatos hallgatás |
| 2026-11-07 | Fixture-generáló: Rust example (WAV + `afconvert` + lofty-tagek) | Csak rendszereszköz kell, a hangfájlok nem kerülnek gitbe |
| 2026-11-07 | Áthelyezés: `renamex_np(…, RENAME_EXCL)` (libc), ENOTSUP-nál (pl. exFAT, hálózati kötet) ellenőrzés + `rename` | Atomi „ne írd felül”; a sima `rename` POSIX-on csendben felülír |
| 2026-11-07 | Köteten átnyúló áthelyezés = másolás + méretellenőrzés + az eredeti eltávolítása | Ez maga az áthelyezés, a fájl a célban megvan; a Kuka itt értelmetlen lenne |
| 2026-11-07 | Másolás: a célnév lefoglalása `O_EXCL`-lel, utána `fs::copy` | Versenyhelyzet nélkül nem ír felül; az APFS-klón így elmarad, de a másolás gyors |
| 2026-11-07 | Az undo napló és a ⌘Z már az M4-ben (nem az M5-ben) | Valódi mappán csak így biztonságos a kézi teszt |
| 2026-11-07 | Másolás visszavonása = a másolat a Kukába | Soha nincs végleges törlés |
| 2026-11-07 | Dönteni csak `pending`, `kept` és `copied` trackről lehet | Ezeknek a fájlja még a forrásban van |
| 2026-11-07 | A fájlművelet a DB-zár nélkül fut, a napló közvetlenül utána, egy tranzakcióban a track új helyével | Egy köteten átnyúló másolás másodpercekig tarthat, a UI-lekérdezések ne álljanak |
| 2026-11-07 | Importált / ráejtett mappa → Setup; Recent session → egyből Triage | A spec: „egy kattintás, és folytatod” |
| 2026-11-07 | A számgombok `key` alapján (nem `code`) | Magyar billentyűzeten a 0 az 1 bal oldalán van |
| 2026-11-07 | Egyszerű Destination bar már az M4-ben (húzás, Move to… az M6-ban) | Látni kell, melyik szám melyik mappa, és kell a visszajelzés |
| 2026-11-07 | A célmappák kimaradnak a scanből | Egy forráson belüli célmappába másolt track ne legyen újra `pending` |
| 2026-11-07 | A lista a track eredeti helye (első akciójának `src`-je) szerint rendez | Áthelyezés és újrascan után se ugráljon a sorrend |
| 2026-11-08 | Azonos track (név + `file_key`) a célban: copy = kihagyja, move = a forrás a Kukába; figyelmeztető sáv | A felhasználó döntése: ne legyen második példány, de feltűnő jelzés kell. Más nevű duplikátumot nem keresünk (spec #19) |
| 2026-11-08 | Kézzel visszatett fájl: a scan egyesíti az eredeti sorral (fájl eltűnt + azonos kulcs + az új sor egy akció `src`-jén áll) | Ne legyen duplikátum, a track maradjon kezelhető; az `src`-feltétel miatt két valódi, azonos tartalmú fájl nem olvad össze |
| 2026-11-08 | A figyelmeztetések borostyánszínűek (`--warning`), a lila marad a hangsúlynak | A spec szerint a lila csak lejátszófej / aktív track / használt csempe |
| 2026-11-08 | Fájlfigyelés: a watcher nem nyúl a DB-hez, csak `folder_changed` eseményt küld, a UI a meglévő scant futtatja | Egyetlen egyeztető logika (scan), nincs kétféle igazság; a scan ~1 ms/fájl |
| 2026-11-08 | notify **8.2** (nem a 9-es RC), FSEvents, mindig rekurzív; az almappák szűrése a mi oldalunkon | Stabil kiadás; az FSEvents amúgy is rekurzív |
| 2026-11-08 | A saját fájlműveletek kiszűrése: `own_op()` őr + 5 mp-ig megjegyzett útvonalak (statikus állapot) | Különben minden döntés újrascant indítana; a köteten átnyúló másolás alatt is visszatartja az eseményeket |
| 2026-11-08 | A forráson belül átnevezett fájl ugyanaz a track (eltűnt sor + új sor azonos `file_key`-jel, az újnak nincs előzménye) | Megmarad a státusz, az undo és a lejátszó aktuális trackje; másolat (az eredeti is megvan) nem olvad össze |
| 2026-11-08 | Eltűnt fájlú, döntött track → `missing` (a sor marad); a célmappában lévő, áthelyezett trackek fájlját nem figyeljük | A naplónak kell a sor; a célmappák figyelése a spec szerint nem szükséges, az undo kezeli az eltűnt fájlt |
| 2026-11-08 | Open in… / jobb klikk: **natív** macOS menü (`@tauri-apps/api/menu`), nem shadcn dropdown | Mac-es érzet, nincs új UI-függőség; a `core:default` már tartalmazza a menü-jogot |
| 2026-11-08 | App-lista: `NSWorkspace.URLsForApplicationsToOpenURL:` (objc2-app-kit), megnyitás `/usr/bin/open -a`; csak a macOS által kínált app fogadható el | Ugyanaz a lista, mint a Finder „Open With”-je; a UI nem indíthat tetszőleges appot |
| 2026-11-08 | Fájlnév: külön **File** oszlop a listában, nem a lejátszósorban | A felhasználó döntése: a lejátszósorba ne kerüljön felesleges infó |
| 2026-11-08 | Undo session a Summary-val együtt v1.1 | Az MVP-ben a többlépéses ⌘Z elég, a napló már megvan hozzá |
| 2026-11-08 | **Spec-módosítás:** Title, Artist és fájlnév helyben szerkeszthető a listában (#23, MVP); teljes tagszerkesztő továbbra sincs | A felhasználó kérése: az alapvető javítások ne igényeljenek külön appot |
| 2026-11-08 | Szerkesztés indítása: katt a már kijelölt sor cellájára (Finder-stílus, 450 ms késleltetés) | A dupla katt = lejátszás, az Enter = Keep foglalt; a felhasználó választotta |
| 2026-11-08 | Tag írása helyben, klón + ellenőrzés + atomi csere, ⌘Z-vel; az eredeti nem megy a Kukába | A felhasználó választotta; a Kukás változat minden szerkesztésnél teljes másolatot halmozna |
| 2026-11-08 | Tag írása a konkrét tagtípuson (ID3v2 / Vorbis / ilst) és fájlszintű mentéssel | A lofty általános `Tag`-je eldobja az ismeretlen frame-eket (Serato cue pontok GEOB-ban); a FLAC-képek csak fájlszintű mentéssel maradnak meg |
| 2026-11-08 | Átnevezésnél foglalt név = hiba, nem `(2)` | Kézi átnevezésnél a felhasználó pontos nevet akar; a `(2)` csak a rendezésnél automatikus |
| 2026-11-08 | `0007`: az `actions` tábla újraépítése (új `kind` értékek: `rename`, `tag`) | SQLite-ban a CHECK nem módosítható; egy napló marad, így a ⌘Z sorrendje egyértelmű |
| 2026-11-07 | Egy alkalmazott migrációt akkor sem módosítunk, ha még nincs commitolva | A futó `tauri dev` a fejlesztői DB-n már lefuttatta (így lett a `0005` külön) |

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
| `src/` | React UI: `lib/trackMenu.ts` (natív jobb klikk / Open in… menü), `App.tsx` (képernyőváltás + drop), `screens/StartScreen.tsx`, `screens/SetupScreen.tsx`, `screens/TriageScreen.tsx` (waveform + lista + döntések), `components/Waveform.tsx`, `components/NowPlaying.tsx`, `components/TrackTable.tsx`, `components/DestinationBar.tsx`, `components/Segmented.tsx`, `components/DropOverlay.tsx`, `state/app.ts` (képernyő), `state/session.ts` (session, trackek, célmappák, scan, döntések), `state/player.ts` (az `<audio>`), `state/keymap.ts` (billentyűk), `lib/api.ts` (típusos commandok + események), `lib/format.ts`, `lib/theme.ts`, `components/ui/` (shadcn) |
| `src-tauri/src/` | Rust: `lib.rs` (setup, DB megnyitása), `commands.rs`, `error.rs`, `finder.rs` (Reveal, Open with), modulok: `library/ audio/ analysis/ fileops/ store/` |
| `src-tauri/src/library/` | `watch.rs` (FSEvents-figyelő, saját műveletek szűrése), `scan.rs` (listázás + szinkron a DB-vel), `tags.rs` (lofty), `file_key.rs`, `testutil.rs` (generált WAV a tesztekhez) |
| `src-tauri/src/store/` | `mod.rs` (megnyitás, migráció), `sessions.rs`, `tracks.rs`, `destinations.rs`, `actions.rs` (undo napló), `peaks.rs`; `migrations/0001`–`0007` |
| `src-tauri/src/fileops/` | `fs.rs` (felülírás nélküli move/copy, klón, Kuka), `mod.rs` (döntés + undo a DB-vel), `edit.rs` (átnevezés, tag írása + undo) |
| `src-tauri/src/audio/` | `mod.rs` (cache + kéréskezelés), `peaks.rs` (symphonia-dekódolás) |
| `src-tauri/examples/make_fixtures.rs` | Tesztkönyvtár-generáló |
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
- A CI első futása zöld.
- **Nyitva:** az ablak szemrevételezése (nem volt képernyőfelvételi engedély).
- **Következő:** M2 (lásd fent).

### 2026-11-07 – M2 import
- Az M1 lezárva: a felhasználó látta a sötét ablakot a három lila pöttyel.
- Rust: `library/` (scan, tagek, `file_key`), `store/sessions.rs` + `tracks.rs`, commandok: `pick_folder`, `open_session`, `recent_sessions`, `set_include_subfolders`, `list_tracks`, `scan_session` (+ `scan_progress` esemény). 19 teszt, clippy és fmt tiszta.
- UI: Start képernyő (Import, ⌘O, drop, Recent sessions), session nézet (fejléc, Include subfolders, progress sáv, újrascan), virtualizált, rendezhető tracklista.
- Fixture-generáló Rust example; a FLAC-ban a BPM `Bpm` kulcsként megy (Vorbis), ezt a generátor mindkét kulccsal írja.
- `tauri dev` elindul, nincs futásidejű hiba; a felületet nem láttam (egy véletlen teljes képernyős felvételt töröltem, nem készítek újat).
- **Nyitva:** kézi teszt az ablakban (lásd „Kézi teszt (M2)”).
- **Következő:** M3 (lásd fent).

### 2026-11-07 – M3 lejátszás
- Az M2 lezárva: a felhasználó kézi tesztje rendben volt.
- Rust: `audio/` (peaks symphoniával, cache, csak a legújabb kérés), `store/peaks.rs` (LRU), `0002` migráció; commandok: `track_peaks`, `set_start_at`, `set_current_track`. 28 teszt, clippy és fmt tiszta.
- UI: wavesurfer 7.12.12 waveform a közös `<audio>`-n, NowPlaying sor (Start at preset), központi keymap, lista kiemelés + kattintás.
- `tauri dev` elindul, futásidejű hiba nincs, a migráció a valódi DB-n lefutott.
- Kézi teszt: minden rendben, de a waveform túl tömör volt (masterelt trackek csúcsa végig teljes kivezérlésen) → RMS-alapú, szellősebb oszlopok tükörképpel (`0003` migráció).
- Az M2 még nincs commitolva; az M2 és az M3 együtt van a munkafában.
- **Nyitva:** az új waveform szemrevételezése.
- **Következő:** M4 (lásd fent).

### 2026-11-07 – M4 döntések
- Rust: `fileops/` (`fs.rs`: `renamex_np` + `RENAME_EXCL`, `(2)` ütközésnél, Kuka objc2-vel; `mod.rs`: `decide`, `undo_last`), `store/destinations.rs`, `store/actions.rs`, `0004`–`0005` migráció; commandok: `set_mode`, `set_auto_next`, `list/add/remove_destination`, `set_destination_hotkey`, `decide`, `undo`; a `pick_folder` címet kaphat. 48 teszt + a valódi Kukás tesztek helyben, clippy és fmt tiszta.
- UI: közös session-store, Setup képernyő, Triage (Enter / Backspace / 1–0 / ⌘Z, auto-next), Destination bar villanással és számlálóval, „done” progress.
- A futó `tauri dev` (a felhasználóé) újrafordult, a migrációk a valódi DB-n lefutottak (séma: 5); a felületet nem láttam.
- Az M3 kézi tesztje (az új waveform szemrevételezése) és az M2–M4 commit még nyitott.
- **Nyitva:** kézi teszt az ablakban (lásd „Kézi teszt (M4)”).
- **Következő:** M5 (lásd fent).

### 2026-11-08 – M4 javítások a kézi teszt után
- A felhasználó: M2, M3 (az új waveformmal) rendben; M4 működik, két hibával.
- Kukából kézzel visszatett track: a scan összeolvasztja az eredeti sorral (`reattach_returned`), a ⌘Z felismeri, ha már a helyén van; a meglévő duplikátum (3 + 101) a valódi DB-n csak olvasva ellenőrizve, a következő scan javítja.
- Azonos track a célban: nem lesz második példány (copy kihagyja, move a forrást a Kukába teszi), borostyán figyelmeztető sáv + villanás; `0006` migráció.
- 54 teszt + 4 Kukás teszt helyben zöld, clippy / fmt / typecheck tiszta; a futó `tauri dev` a 6-os sémán.
- A felhasználó a javításokat kipróbálta, rendben: **az M4 kész.**
- Commit `f8e2ab0` (M2–M4 együtt) a main-re, push; a CI zöld (run 42477560488).
- **Következő:** M5 (fájlfigyelés, Reveal in Finder).

### 2026-11-08 – M5 biztonság
- Rust: `library/watch.rs` (notify 8.2, debounce, `folder_changed`, saját műveletek szűrése), `finder.rs` (Reveal, appok listája objc2-app-kittel, megnyitás), `tracks::relink_renamed` + `mark_missing` a scanben; commandok: `watch_session`, `unwatch_session`, `reveal_track`, `track_apps`, `open_track`. 62 teszt, clippy és fmt tiszta.
- UI: figyelés a session megnyitásakor, csendes újrascan Finder-változásra, a lejátszó követi az átnevezett tracket, ⌘R, Reveal + Open in… gomb, natív jobb klikkes menü, értesítés az eltűnt fájlokról.
- Typecheck tiszta; a felületet nem láttam, a futó `tauri dev` (a felhasználóé) magától újrafordul.
- Undo session → v1.1.
- Kézi teszt (a felhasználó): behúzás, Kuka → visszahúzás (nincs duplikátum), jobb klikk, ⌘R, Open in… rendben. Az átnevezést az app követte (a DB-ben az új útvonal), de a listában nem látszott, mert a Title a tagből jön → **File oszlop**.
- A felhasználó: a File oszlop rendben; M5 commit `6d986df` (helyben, nincs pusholva).
- **Következő:** M6 (lásd fent).

### 2026-11-08 – M5b helyben szerkesztés
- A felhasználó kérése: Title / Artist / fájlnév átírása a listában. Döntések (a felhasználóé): katt a kijelölt sorra; tag helyben + ⌘Z; mezők: Title, Artist, File. A spec módosítva (#23).
- Rust: `fileops/edit.rs`, `tags::write_text` (konkrét tagtípusok), `fs::clone_file`, `0007` migráció, `edit_track` command. 70 teszt + a helyi formátum- és borító/Serato-tesztek zöldek, clippy / fmt / typecheck tiszta.
- UI: szerkeszthető cellák a `TrackTable`-ben, undo-üzenetek, a lejátszó újratöltése ugyanonnan.
- A felhasználó kézi tesztje: minden működik. **Az M5b kész.** Commit `6d986df` (M5) + `190c26f` (M5b), push; a CI zöld (run 42485134010).
- **Következő:** M6.
