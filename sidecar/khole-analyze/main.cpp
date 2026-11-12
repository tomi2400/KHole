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

#include <algorithm>
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
static const char* kAnalyzerVersion = "0.2.0";

static std::string jsonEscape(const std::string& s) {
  std::string out;
  for (char c : s) {
    if (c == '"' || c == '\\') out += '\\';
    out += c;
  }
  return out;
}

// Onset envelope of the low band (the kick), as half-wave rectified log-energy flux at
// about 400 Hz.
static std::vector<double> kickEnvelope(const std::vector<Real>& x, double sr, double* envRate) {
  const size_t hop = static_cast<size_t>(std::lround(sr / 400.0));
  *envRate = sr / hop;
  const double a = std::exp(-2.0 * M_PI * 150.0 / sr);  // two one-pole low-passes at 150 Hz
  double y1 = 0, y2 = 0, acc = 0, prev = 0;
  size_t k = 0;
  std::vector<double> odf;
  odf.reserve(x.size() / hop + 1);
  for (Real v : x) {
    y1 = (1 - a) * v + a * y1;
    y2 = (1 - a) * y1 + a * y2;
    acc += y2 * y2;
    if (++k == hop) {
      double l = std::log(1e-10 + acc);
      odf.push_back(odf.empty() ? 0.0 : std::max(0.0, l - prev));
      prev = l;
      acc = 0;
      k = 0;
    }
  }
  return odf;
}

// Strength of a steady pulse at `bpm` in the envelope: the power of its DFT at the beat
// frequency and two harmonics (each one a beat-synchronous phase sum over the whole track).
static double pulseStrength(const std::vector<double>& odf, double envRate, double bpm) {
  double total = 0;
  for (int h : {1, 2, 4}) {
    const double w = 2.0 * M_PI * h * (bpm / 60.0) / envRate;
    const double cw = std::cos(w), sw = std::sin(w);
    double c = 1, s = 0, re = 0, im = 0;
    for (size_t n = 0; n < odf.size(); ++n) {
      re += odf[n] * c;
      im += odf[n] * s;
      const double nc = c * cw - s * sw;
      s = s * cw + c * sw;
      c = nc;
      if ((n & 1023) == 1023) {  // keep the rotation on the unit circle
        const double r = std::sqrt(c * c + s * s);
        c /= r;
        s /= r;
      }
    }
    total += (re * re + im * im) / h;
  }
  return total;
}

// Electronic music keeps one tempo for minutes, so the right tempo is the one at which the
// kicks of the whole track line up: search Essentia's estimate ±4 BPM for the strongest
// pulse, coarse then fine. Over a 6-minute track this resolves well under 0.05 BPM.
static double refineTempo(const std::vector<Real>& audio, double sr, double estimate) {
  double envRate = 0;
  const std::vector<double> odf = kickEnvelope(audio, sr, &envRate);
  if (odf.size() < envRate * 10) return estimate;  // under 10 s: keep Essentia's
  auto search = [&](double lo, double hi, double step) {
    double best = estimate, bestScore = -1;
    for (double b = lo; b <= hi + 1e-9; b += step) {
      const double sc = pulseStrength(odf, envRate, b);
      if (sc > bestScore) {
        bestScore = sc;
        best = b;
      }
    }
    return best;
  };
  const double coarse = search(estimate - 4.0, estimate + 4.0, 0.05);
  return search(coarse - 0.06, coarse + 0.06, 0.002);
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

    // Essentia's estimate is quantized by its onset frames (512-sample hops: a 160 BPM track
    // reads 161.5, i.e. 32 frames per beat). Refine it on the whole track instead: see
    // refineTempo.
    Real refined = bpm > 0 ? static_cast<Real>(refineTempo(audio, kTargetSr, bpm)) : bpm;

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
