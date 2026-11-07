// khole-analyze: BPM + key analysis sidecar for KHole.
//
// Reads raw mono 32-bit float little-endian PCM from stdin (decoded by the Rust side
// with symphonia, so no ffmpeg is needed here) and prints one JSON object to stdout:
//   {"bpm":124.0,"bpm_raw":123.58,"bpm_confidence":3.81,"key":"A","scale":"minor","key_strength":0.85}
//
// Usage: khole-analyze --sr <input sample rate> [--method multifeature|degara]
//        khole-analyze --version   -> {"name":"khole-analyze","version":"...","essentia":"..."}
// Exit codes: 0 ok, 1 bad arguments, 2 no/too little audio, 3 analysis error.
//
// License: GPL-3.0 (links Essentia, AGPL-3.0).

#include <essentia/algorithmfactory.h>
#include <essentia/essentia.h>

#include <cmath>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

using namespace essentia;
using namespace essentia::standard;

static const Real kTargetSr = 44100.0;

// Bump whenever the analysis output can change: cached results are keyed on it.
static const char* kAnalyzerVersion = "0.1.0";

static std::string jsonEscape(const std::string& s) {
  std::string out;
  for (char c : s) {
    if (c == '"' || c == '\\') out += '\\';
    out += c;
  }
  return out;
}

int main(int argc, char** argv) {
  Real inputSr = 0;
  std::string method = "multifeature";
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--version") {
      std::cout << "{\"name\":\"khole-analyze\",\"version\":\"" << kAnalyzerVersion
                << "\",\"essentia\":\"" << jsonEscape(essentia::version) << "\"}" << std::endl;
      return 0;
    }
    if (a == "--sr" && i + 1 < argc) inputSr = std::stof(argv[++i]);
    else if (a == "--method" && i + 1 < argc) method = argv[++i];
    else {
      std::cerr << "usage: khole-analyze --sr <rate> [--method multifeature|degara]\n";
      return 1;
    }
  }
  if (inputSr <= 0 || (method != "multifeature" && method != "degara")) {
    std::cerr << "usage: khole-analyze --sr <rate> [--method multifeature|degara]\n";
    return 1;
  }

  // Read all of stdin as f32le.
  std::vector<Real> audio;
  {
    std::vector<float> chunk(1 << 16);
    size_t n;
    while ((n = std::fread(chunk.data(), sizeof(float), chunk.size(), stdin)) > 0)
      audio.insert(audio.end(), chunk.begin(), chunk.begin() + n);
  }
  if (audio.size() < static_cast<size_t>(inputSr * 5)) {  // under 5 s: not worth analysing
    std::cerr << "too little audio\n";
    return 2;
  }

  try {
    essentia::init();
    essentia::warningLevelActive = false;
    essentia::infoLevelActive = false;
    AlgorithmFactory& f = AlgorithmFactory::instance();

    if (std::fabs(inputSr - kTargetSr) > 0.5) {
      Algorithm* rs = f.create("Resample", "inputSampleRate", inputSr, "outputSampleRate", kTargetSr);
      std::vector<Real> resampled;
      rs->input("signal").set(audio);
      rs->output("signal").set(resampled);
      rs->compute();
      delete rs;
      audio.swap(resampled);
    }

    Real bpm = 0, confidence = 0;
    std::vector<Real> ticks, estimates, intervals;
    Algorithm* rhythm = f.create("RhythmExtractor2013", "method", method);
    rhythm->input("signal").set(audio);
    rhythm->output("bpm").set(bpm);
    rhythm->output("ticks").set(ticks);
    rhythm->output("confidence").set(confidence);
    rhythm->output("estimates").set(estimates);
    rhythm->output("bpmIntervals").set(intervals);
    rhythm->compute();
    delete rhythm;

    // Refine the tempo from the beat positions: least-squares fit of tick time vs. beat index.
    // The slope is the beat period; this is more precise than the histogram-based estimate,
    // which tends to read ~0.5 BPM low on steady electronic music.
    // The window grows (32, 64, 128 ... ticks) so the beat indices never drift by a whole beat
    // on long tracks; ticks off the grid (missed or extra beats) are left out of each fit.
    Real refined = bpm;
    if (ticks.size() >= 16 && bpm > 0) {
      double period = 60.0 / bpm;
      const double t0 = ticks[0];
      size_t end = std::min<size_t>(32, ticks.size());
      while (true) {
        double n = 0, sx = 0, sy = 0, sxx = 0, sxy = 0;
        for (size_t i = 0; i < end; ++i) {
          double idx = std::round((ticks[i] - t0) / period);
          if (std::fabs(ticks[i] - t0 - idx * period) < period * 0.15) {
            n += 1; sx += idx; sy += ticks[i]; sxx += idx * idx; sxy += idx * ticks[i];
          }
        }
        double denom = n * sxx - sx * sx;
        if (n < 16 || denom <= 0) break;
        period = (n * sxy - sx * sy) / denom;
        if (end == ticks.size()) break;
        end = std::min(end * 2, ticks.size());
      }
      double candidate = 60.0 / period;
      if (std::fabs(candidate - bpm) < 2.0) refined = candidate;  // sanity: stay near Essentia's estimate
    }

    std::string key, scale;
    Real strength = 0;
    Algorithm* keyAlg = f.create("KeyExtractor", "profileType", std::string("edma"), "sampleRate", kTargetSr);
    keyAlg->input("audio").set(audio);
    keyAlg->output("key").set(key);
    keyAlg->output("scale").set(scale);
    keyAlg->output("strength").set(strength);
    keyAlg->compute();
    delete keyAlg;

    essentia::shutdown();

    std::printf(
        "{\"bpm\":%.2f,\"bpm_essentia\":%.2f,\"bpm_confidence\":%.2f,\"key\":\"%s\",\"scale\":\"%s\",\"key_strength\":%.2f}\n",
        refined, bpm, confidence, jsonEscape(key).c_str(), jsonEscape(scale).c_str(), strength);
    return 0;
  } catch (const std::exception& e) {
    std::cerr << "analysis error: " << e.what() << "\n";
    return 3;
  }
}
