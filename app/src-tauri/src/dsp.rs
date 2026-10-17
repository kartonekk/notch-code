//! Small audio helpers: 48 kHz stereo -> 16 kHz mono, and an energy based
//! voice activity segmenter that turns a continuous stream into utterances.

use std::collections::VecDeque;

pub const RATE: usize = 16_000;
const FRAME: usize = RATE / 50; // 20 ms

/// Stereo f32 @ 48 kHz -> mono f32 @ 16 kHz (windowed-sinc low-pass + decimate by 3).
pub struct Downsampler {
    taps: Vec<f32>,
    hist: Vec<f32>,
    phase: usize,
}

/// `n`-tap Blackman windowed sinc low-pass, cutoff `fc` as a fraction of the sample rate.
fn lowpass(n: usize, fc: f32) -> Vec<f32> {
    let m = (n - 1) as f32;
    let mut taps: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f32 - m / 2.0;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * std::f32::consts::PI * fc * x).sin() / (std::f32::consts::PI * x)
            };
            let w = 0.42 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / m).cos()
                + 0.08 * (4.0 * std::f32::consts::PI * i as f32 / m).cos();
            sinc * w
        })
        .collect();
    let sum: f32 = taps.iter().sum();
    taps.iter_mut().for_each(|t| *t /= sum);
    taps
}

impl Downsampler {
    pub fn new() -> Self {
        // 63-tap Blackman windowed sinc, cutoff 7.2 kHz @ 48 kHz
        let n = 63;
        Self { hist: vec![0.0; n], taps: lowpass(n, 7_200.0 / 48_000.0), phase: 0 }
    }

    /// `interleaved` is stereo f32 at 48 kHz. Output is appended to `out`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        let n = self.taps.len();
        for frame in interleaved.chunks_exact(2) {
            let mono = 0.5 * (frame[0] + frame[1]);
            self.hist.rotate_left(1);
            self.hist[n - 1] = mono;
            self.phase += 1;
            if self.phase == 3 {
                self.phase = 0;
                let y: f32 = self.hist.iter().zip(&self.taps).map(|(a, b)| a * b).sum();
                out.push(y);
            }
        }
    }
}

/// Mono f32 at any sample rate -> 16 kHz (recorded files): low-pass below
/// 8 kHz when downsampling, then linear interpolation.
pub struct Resampler {
    taps: Vec<f32>,
    /// input not consumed yet; output n reads `buf[pos..pos + taps.len() + 1]`
    buf: Vec<f32>,
    pos: f64,
    step: f64,
}

impl Resampler {
    pub fn new(rate: u32) -> Self {
        let taps = if rate as usize > RATE { lowpass(63, 7_200.0 / rate as f32) } else { vec![1.0] };
        Self { buf: vec![0.0; taps.len() - 1], taps, pos: 0.0, step: rate as f64 / RATE as f64 }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        let n = self.taps.len();
        let filtered = |at: usize| -> f32 { self.buf[at..at + n].iter().zip(&self.taps).map(|(a, b)| a * b).sum() };
        loop {
            let i = self.pos as usize;
            if i + n + 1 > self.buf.len() {
                break;
            }
            let frac = (self.pos - i as f64) as f32;
            let (a, b) = (filtered(i), filtered(i + 1));
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        let used = (self.pos as usize).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
    }
}

pub enum VadEvent {
    /// Someone is still talking; `audio` is everything said so far in this utterance.
    Partial { utt: u64, start_ms: u64, audio: Vec<f32> },
    /// The utterance ended (silence or max length).
    Final { utt: u64, start_ms: u64, audio: Vec<f32> },
}

pub struct Segmenter {
    min_threshold: f32,
    noise: f32,
    frame: Vec<f32>,
    preroll: VecDeque<f32>,
    speaking: bool,
    speech_run: usize,
    silence_run: usize,
    utt: Vec<f32>,
    utt_id: u64,
    utt_start_ms: u64,
    last_partial_len: usize,
    partials: bool,
    end_frames: usize,
    max_utt: usize,
    /// samples received so far; drives the utterance timestamps
    clock: u64,
    /// offset of this stream relative to the recording start
    offset_ms: u64,
}

const START_FRAMES: usize = 3; // 60 ms of speech opens an utterance
const END_FRAMES: usize = 35; // 700 ms of silence closes it
const DICTATION_END_FRAMES: usize = 250; // 5 s: people pause while thinking up a prompt
const KEEP_TAIL: usize = RATE / 5; // keep 200 ms of the trailing silence
const PREROLL: usize = RATE * 3 / 10; // 300 ms before the onset
const MAX_UTT: usize = RATE * 20; // force a cut after 20 s
const DICTATION_MAX_UTT: usize = RATE * 60;
const MIN_UTT: usize = RATE * 2 / 5; // ignore blips shorter than 400 ms
const PARTIAL_EVERY: usize = RATE; // live preview every 1 s of new audio

impl Segmenter {
    /// `min_threshold` is the minimum RMS considered speech (app loopback is
    /// digitally silent, a microphone has a noise floor).
    pub fn new(min_threshold: f32, partials: bool, offset_ms: u64) -> Self {
        Self {
            min_threshold,
            noise: 1e-4,
            frame: Vec::with_capacity(FRAME),
            preroll: VecDeque::with_capacity(PREROLL),
            speaking: false,
            speech_run: 0,
            silence_run: 0,
            utt: Vec::new(),
            utt_id: 0,
            utt_start_ms: 0,
            last_partial_len: 0,
            partials,
            end_frames: END_FRAMES,
            max_utt: MAX_UTT,
            clock: 0,
            offset_ms,
        }
    }

    /// Spoken prompts: longer pauses and longer utterances before cutting.
    pub fn for_dictation(mut self) -> Self {
        self.end_frames = DICTATION_END_FRAMES;
        self.max_utt = DICTATION_MAX_UTT;
        self
    }

    pub fn push(&mut self, samples: &[f32], next_id: &mut dyn FnMut() -> u64, out: &mut Vec<VadEvent>) {
        for &s in samples {
            self.frame.push(s);
            if self.frame.len() == FRAME {
                let frame = std::mem::take(&mut self.frame);
                self.on_frame(&frame, next_id, out);
                self.frame = frame;
                self.frame.clear();
            }
        }
    }

    /// Emit whatever is buffered as a final utterance (used when stopping).
    pub fn flush(&mut self, out: &mut Vec<VadEvent>) {
        if self.speaking {
            self.finish(out, false);
        }
    }

    fn now_ms(&self) -> u64 {
        self.offset_ms + self.clock * 1000 / RATE as u64
    }

    fn on_frame(&mut self, frame: &[f32], next_id: &mut dyn FnMut() -> u64, out: &mut Vec<VadEvent>) {
        self.clock += frame.len() as u64;
        let rms = (frame.iter().map(|x| x * x).sum::<f32>() / frame.len() as f32).sqrt();
        let threshold = (self.noise * 3.0).max(self.min_threshold);
        let is_speech = rms > threshold;

        if !self.speaking {
            if !is_speech {
                self.noise = (self.noise * 0.98 + rms * 0.02).max(1e-4);
            }
            self.preroll.extend(frame);
            while self.preroll.len() > PREROLL + FRAME * START_FRAMES {
                self.preroll.pop_front();
            }
            self.speech_run = if is_speech { self.speech_run + 1 } else { 0 };
            if self.speech_run >= START_FRAMES {
                self.speaking = true;
                self.silence_run = 0;
                self.utt.clear();
                self.utt.extend(self.preroll.drain(..));
                self.utt_id = next_id();
                self.utt_start_ms = self.now_ms().saturating_sub((self.utt.len() * 1000 / RATE) as u64);
                self.last_partial_len = 0;
            }
            return;
        }

        self.utt.extend_from_slice(frame);
        self.silence_run = if is_speech { 0 } else { self.silence_run + 1 };

        if self.silence_run >= self.end_frames {
            let trailing = self.silence_run * FRAME;
            let cut = trailing.saturating_sub(KEEP_TAIL);
            self.utt.truncate(self.utt.len() - cut.min(self.utt.len()));
            self.finish(out, false);
        } else if self.utt.len() >= self.max_utt {
            self.finish(out, true);
            // keep talking: the next utterance starts right here
            self.speaking = true;
            self.utt_id = next_id();
            self.utt_start_ms = self.now_ms();
        } else if self.partials && self.utt.len() - self.last_partial_len >= PARTIAL_EVERY {
            self.last_partial_len = self.utt.len();
            out.push(VadEvent::Partial {
                utt: self.utt_id,
                start_ms: self.utt_start_ms,
                audio: self.utt.clone(),
            });
        }
    }

    fn finish(&mut self, out: &mut Vec<VadEvent>, forced: bool) {
        self.speaking = false;
        self.speech_run = 0;
        self.preroll.clear();
        let audio = std::mem::take(&mut self.utt);
        if audio.len() >= MIN_UTT || forced {
            out.push(VadEvent::Final {
                utt: self.utt_id,
                start_ms: self.utt_start_ms,
                audio,
            });
        } else if self.last_partial_len > 0 {
            // a preview was shown for it; tell the UI it turned out to be nothing
            out.push(VadEvent::Final { utt: self.utt_id, start_ms: self.utt_start_ms, audio: Vec::new() });
        }
    }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}
