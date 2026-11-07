# KHole – Spike eredmények (2026-11-07)

Gép: Apple Silicon (arm64), macOS 26.5.2, Rust 1.99, Tauri 2, WebKit 605.1.15.
Tesztfájlok: `spike/fixtures/` (generált 124 BPM-es kick + A-moll akkord, 44,1 kHz, 24 bit; 30 s és 10 perc).
Kód: `spike/probe/` (Rust) és `spike/playback/` (Tauri). Eldobható, nem kerül az appba.

## 1. Lejátszás a WebView-ban (asset protokoll + `<audio>`)

| Fájl | Metaadat betöltése | Lejátszás | Tekerés 500 s-ra |
|---|---|---|---|
| AIFF 10 perc, 24 bit | 100 ms | ok | ok, 130 ms |
| WAV 10 perc, 24 bit | 129 ms | ok | ok, 87 ms |
| AIFF / WAV 30 s | 107–128 ms | ok | – |
| FLAC | 111 ms | ok | – |
| MP3 320 kbps | 13 ms | ok | – |
| AAC M4A | 132 ms | ok | – |
| ALAC M4A | 92 ms | ok | – |

**Döntés:** minden célformátum natívan szól, a seek nagy fájlban is gyors. Rust oldali dekódolás lejátszáshoz **nem kell**. Az AIFF nyitott kérdés lezárva.
Az `assetProtocol.scope` a futás közben kiválasztott mappákra állítandó (`FsScope::allow_directory`).

## 2. Waveform-csúcsok (symphonia 0.6, 100 csúcs/s, mono min/max)

| Fájl | Idő |
|---|---|
| AIFF 10 perc, 24 bit | 164 ms |
| WAV 10 perc, 24 bit | 151 ms |
| MP3 30 s | 23 ms (kb. 0,3 s egy 7 perces tracknél) |
| FLAC / AAC / ALAC 30 s | 14 / 34 / 50 ms |

**Döntés:** a csúcsokat a Rust számolja, cache-eli, és a wavesurfer `peaks` + `duration` formában kapja meg. A cél (< 1 s) bőven teljesül. Release build kell, debugban sokkal lassabb.

## 3. Tagek (lofty 0.25)
Minden formátum olvasható, < 1 ms / fájl (hossz, bitráta, sample rate, bitmélység, tagek). 480 track kb. fél másodperc.

## 4. BPM / key – Essentia (PyPI wheel 2.1b6, mérés céljából)

| Fájl | BPM (RhythmExtractor2013, multifeature) | Key (KeyExtractor, `edma`) |
|---|---|---|
| Mind a 6 formátum, 30 s | 123,4–123,6 (valós: 124), kb. 0,3 s | A minor, 0,02 s |
| WAV 10 perc | 123,7, 5,2 s | A minor, 0,2 s |

**Megállapítások:**
- A key pontos. Az Essentia BPM-je 0,3–0,5-tel alacsonyabb, és sima kerekítéssel hol 123, hol 124 lesz belőle.
- Sebesség: a teljes trackes multifeature kb. 0,5 s / perc. 480 track 4 szálon nagyjából 5–7 perc. Gyorsítás, ha kell: csak a track középső 2 percét elemezzük, vagy `degara` módszer.

### Sidecar: `sidecar/khole-analyze` (elkészült)
- **Build:** `sidecar/khole-analyze/build.sh <essentia-forrás>`. Statikus, minimális Essentia (git HEAD 23ae7ba): `--lightweight=libsamplerate --fft=ACCELERATE`. Nem kell hozzá ffmpeg, fftw, yaml vagy taglib. Az Eigen 5.0.1 (Homebrew) gond nélkül lefordul.
- **Kimenet:** `src-tauri/binaries/khole-analyze-aarch64-apple-darwin`, 7,4 MB, és csak rendszerkönyvtárakat linkel (Accelerate, libc++, libSystem).
- **Interfész:** stdinen mono f32le PCM (a Rust/symphonia dekódol), `--sr <rate>`; nem 44,1 kHz esetén a sidecar maga mintavételez át. Kimenet egy JSON sor: `{"bpm":124.00,"bpm_essentia":123.68,"bpm_confidence":3.76,"key":"A","scale":"minor","key_strength":0.85}`. Kilépési kódok: 1 rossz argumentum, 2 túl rövid (< 5 s), 3 elemzési hiba.
- **BPM-finomítás:** egyenes illesztése az ütésidőpontokra, fokozatosan bővülő ablakkal (32 → 64 → … ütés), a rácsról lelógó ütések nélkül. Eredmény minden teszten: **124,00 ± 0,01** (MP3/FLAC/AIFF/WAV/AAC/ALAC, 48 kHz-es FLAC, 10 perces fájl). A UI egy tizedesre kerekítve mutatja.
- **Teljes idő** (dekódolás + elemzés): 30 s-os track kb. 0,4 s, 10 perces kb. 6 s.
- **Fenntartás:** eddig csak a generált, egyenletes tesztjelen mértünk. Az M7-ben valódi trackeken is ellenőrizni kell (tempóváltás, breakdown, half-time), és ugyanott beállítani a technóra hangolt tartományt (#16).

## 5. Kukába dobás + visszaállítás
- A `trash` crate macOS-en **nem** adja vissza a Kukában lévő útvonalat, az alapmódja pedig Finder/AppleScript (automatizálási engedélyt kér).
- **Döntés:** saját hívás `objc2-foundation` → `NSFileManager.trashItemAtURL:resultingItemURL:`. Tesztelve: 70 ms, visszaadja a `~/.Trash/...` útvonalat, és ékezetes névvel is visszaállítható. Az undo naplóba ez az útvonal kerül.
