//! Decoding to a mono f32 stream with symphonia: the one decoder behind the waveform peaks
//! and the analysis (the sidecar gets this stream on its stdin).

use std::fs::File;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::error::{Error, Result};

pub struct MonoDecoder {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    channels: usize,
}

impl MonoDecoder {
    /// Opens and probes the file; nothing is decoded yet.
    pub fn open(path: &Path) -> Result<Self> {
        let undecodable = |e: SymError| Error::Decode(format!("{}: {e}", path.display()));

        let file = File::open(path).map_err(|_| Error::NotFound(path.display().to_string()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(undecodable)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| Error::Decode(format!("{}: no audio track", path.display())))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| Error::Decode(format!("{}: unsupported codec", path.display())))?
            .clone();
        let track_id = track.id;
        let rate = params
            .sample_rate
            .ok_or_else(|| Error::Decode(format!("{}: unknown sample rate", path.display())))?;
        let channels = params.channels.as_ref().map_or(2, |c| c.count()).max(1);
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(undecodable)?;
        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            rate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Decodes packet by packet, handing each block of mono samples (the mean of the
    /// channels) to `sink`; `sink` may stop early with `ControlFlow::Break`. `cancelled` is
    /// polled between packets and ends the run with [`Error::Cancelled`]. Returns the number
    /// of frames handed over; a file without a single frame is an error.
    pub fn run(
        mut self,
        cancelled: impl Fn() -> bool,
        mut sink: impl FnMut(&[f32]) -> Result<ControlFlow<()>>,
    ) -> Result<u64> {
        let undecodable = |e: SymError| Error::Decode(format!("{}: {e}", self.path.display()));
        let channels = self.channels;
        let mut interleaved: Vec<f32> = Vec::new();
        let mut mono: Vec<f32> = Vec::new();
        let mut frames = 0u64;

        loop {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => break,
                // A truncated file still gives what is there.
                Err(SymError::IoError(_)) => break,
                Err(e) => return Err(undecodable(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(_)) => continue, // skip a corrupt packet
                Err(e) => return Err(undecodable(e)),
            };
            interleaved.resize(decoded.samples_interleaved(), 0.0);
            decoded.copy_to_slice_interleaved(&mut interleaved);
            mono.clear();
            mono.extend(
                interleaved
                    .chunks_exact(channels)
                    .map(|f| f.iter().sum::<f32>() / channels as f32),
            );
            frames += mono.len() as u64;
            if sink(&mono)?.is_break() {
                break;
            }
        }
        if frames == 0 {
            return Err(Error::Decode(format!(
                "{}: no audio frames",
                self.path.display()
            )));
        }
        Ok(frames)
    }
}
