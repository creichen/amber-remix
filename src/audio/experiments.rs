// Copyright (C) 2024 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.


#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use hound::WavWriter;
use std::{collections::{VecDeque, HashSet, HashMap}, sync::{Mutex, Arc}, fs::File, io::BufWriter, mem, fmt::Display};
use lazy_static::lazy_static;
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use rustfft::{FftPlanner, num_complex::Complex, FftDirection};
//use sdl2::libc::STA_FREQHOLD;
use crate::{datafiles::{music::Song, sampledata::SampleData}, audio::{AQOp, AudioIterator}};
use super::{amber::SongIterator, AQSample, SampleRange, dsp::streamlog::{StreamLogger, self, StreamLogClient}};
use super::blep::BLEP;
use super::acore::AudioSource;

// fn gen_wave(bytes_to_write: i32) -> Vec<i16> {
//     // Generate a square wave
//     let tone_volume = 10_000i16;
//     let period = 48_000 / 256;
//     let sample_count = bytes_to_write;
//     let mut result = Vec::new();

//     for x in 0..sample_count {
//         result.push(if (x / period) % 2 == 0 {
//             tone_volume
//         } else {
//             -tone_volume
//         });
//     }
//     result
// }

pub const SAMPLE_RATE : usize = 48_000;

// ================================================================================
// SampleProvider

#[derive(Hash, PartialEq, Eq)]
struct ResamplingKey {
    range: SampleRange,
    freq: usize,
    looping: bool,
}

impl ResamplingKey {
    fn new(range: SampleRange,
	   freq: usize,
	   looping: bool) -> Self {
	ResamplingKey {
	    range,
	    freq,
	    looping,
	}
    }

}

struct SampleProvider {
    sample_data: SampleData,
    looping_samples: Vec<SampleRange>,
    nonlooping_samples: Vec<SampleRange>,
    //scratch: Vec<Complex<f32>>,
    samples: HashMap<ResamplingKey, Vec<f32>>,
    empty: Vec<f32>,
}

impl SampleProvider {
    fn new(sample_data: &SampleData, songs: &[Song]) -> Self {
	let mut non_looping_samples = HashSet::new();
	let mut looping_samples = HashSet::new();
	for song in songs {
	    let (nl, l) = song.samples();
	    for s in nl {
		non_looping_samples.insert(s);
	    }
	    for s in l {
		looping_samples.insert(s);
	    }
	}
	let mut nl_samples : Vec<SampleRange> = non_looping_samples.iter().map(|x| *x).collect();
	let mut l_samples : Vec<SampleRange> = looping_samples.iter().map(|x| *x).collect();
	nl_samples.sort();
	l_samples.sort();

	let mut result = SampleProvider {
	    sample_data: sample_data.clone(),
	    nonlooping_samples: nl_samples.clone(),
	    looping_samples: l_samples.clone(),
	    //scratch: vec![],
	    samples: HashMap::new(),
	    empty: vec![],
	};
	result.resample_looping(&l_samples);
	result.resample_nonlooping(&nl_samples);
	result
    }

    fn resample_looping(&mut self, samples: &Vec<SampleRange>) {
	for &sample_range in samples {
	    self.samples.insert(ResamplingKey::new(sample_range, 0, false),
				self.sample_f32(sample_range));
	}
    }
    fn resample_nonlooping(&mut self, samples: &Vec<SampleRange>) {
	// FIXME
	self.resample_looping(samples);
    }

    // fn ensure_scratch(&mut self, len: usize) {
    // 	if self.scratch.len() < len {
    // 	    self.scratch = vec![Complex::new(0.0, 0.0); len];
    // 	}
    // }

    fn sample_f32(&self, range: SampleRange) -> Vec<f32> {
	return self.sample_data[range].iter().map(|&v| v as f32 / 128.0).collect();
    }

    fn next_lowest_freq(&self, _freq: usize) -> usize {
	0
    }

    fn get(&self, range: SampleRange, freq: usize, _is_looping: bool) -> &[f32] {
	return &self.samples[&ResamplingKey::new(range,
						 self.next_lowest_freq(freq),
						 false)];
    }

    fn empty_sample(&self) -> &[f32] {
	&self.empty
    }
}

impl Display for SampleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	//let sep: String = ", ".into();
	let l : String = self.looping_samples.iter().map(|x| x.show()).collect();
	let nl : String = self.nonlooping_samples.iter().map(|x| x.show()).collect();
	writeln!(f, "[SampleProvider]\n      looping: ({}) {l}\n  non-looping: ({}) {nl}",
		 self.looping_samples.len(),
		 self.nonlooping_samples.len(),
	)
    }
}



// ================================================================================
// Instruments
#[derive(Clone, Debug)]
enum InstrumentUpdate<'a> {
    New(&'a [f32], bool), // bool: sliding (i.e., preserve offset)
    Loop,
    None
}

#[derive(Clone)]
struct Instrument {
    ops: Vec<AQSample>,
    last_range: Option<SampleRange>,
    // Deliberately NOT embedding sample data reference to avoid polluting with
    // a lifetime modifier (which would then mess up memory management later)
}

impl Instrument {
    fn new(ops: Vec<AQSample>) -> Self {
	Instrument {
	    ops,
	    last_range: None,
	}
    }

    fn empty() -> Self {
	Instrument {
	    ops: vec![],
	    last_range: None,
	}
    }

    fn is_looping(&self) -> bool {
	match self.ops.as_slice() {
	    [AQSample::Loop(_), ..] => true,
	    _ => false,
	}
    }

    fn is_sliding(&self) -> bool {
	match self.ops.as_slice() {
	    [AQSample::OnceAtOffset(_, _), ..] => true,
	    _ => false,
	}
    }

    fn current_sample_range(&self) -> Option<SampleRange> {
	match self.ops.as_slice() {
	    [AQSample::Once(r), ..]            => Some(*r),
	    [AQSample::Loop(r), ..]            => Some(*r),
	    [AQSample::OnceAtOffset(r, _), ..] => Some(*r),
	    _  => None,
	}
    }

    fn current_sample<'a>(&self, sample_provider: &'a SampleProvider, freq: usize) -> &'a [f32] {
	match self.current_sample_range() {
	    None => &sample_provider.empty_sample(),
	    Some(r) => sample_provider.get(r, freq, self.is_looping()),
	}
    }

    // fn current_sample(&self, sample_provider: &SampleProvider, freq: usize) -> Vec<f32> {
    // 	match self.current_sample_raw(sample_provider, freq) {
    // 	    None => vec![],
    // 	    Some(pcm) => pcm.iter()
    // 		.map(|&v| v as f32 / 128.0).collect(),
    // 	}
    // }

    fn next_sample<'a>(&mut self, sample_provider: &'a SampleProvider, freq: usize) -> InstrumentUpdate<'a> {
	let new_range = self.current_sample_range();
	if new_range.is_none() {
	    return InstrumentUpdate::None;
	}
	if new_range == self.last_range {
	    trace!("    -> looping sample: {:?}", self.current_sample_range());
	    return InstrumentUpdate::Loop;
	}
	// otherwise we have an actual update
	let sample = self.current_sample(sample_provider, freq);
	trace!("    -> single sample: {:?}", self.current_sample_range());
	if !self.is_looping() {
	    self.ops = self.ops[1..self.ops.len()].to_vec();
	}
	return InstrumentUpdate::New(sample, self.is_sliding());
    }
}

// ================================================================================

fn mk_sine(buf: &mut [f32], freq : usize) {
    for x in 0 .. buf.len() {
	let pos = x;
	let sine = f32::sin((pos as f32) * 2.0 * 3.1415 * (freq as f32) / (SAMPLE_RATE as f32));
	buf[pos] = sine;
    }
}

lazy_static! {
    static ref BLEPPER: BLEP = BLEP::new();
}

// ================================================================================
#[derive(Clone)]
struct ChannelState {
    freq : usize,
    volume: f32,
    instrument: Instrument,
}

impl<'a> ChannelState {
    fn new() -> Self {
	ChannelState {
	    freq: 0,
	    volume: 0.0,
	    instrument: Instrument::empty(),
	}
    }

    fn resample_ratio(&self) -> f64 {
	let sample_rate = SAMPLE_RATE as f64;
	let frequency = self.freq as f64;
	return sample_rate / frequency;
    }

    fn inv_resample_ratio(&self) -> f64 {
	let sample_rate = SAMPLE_RATE as f64;
	let frequency = self.freq as f64;
	return frequency / sample_rate;
    }

    fn next_instrument_sample(&mut self, sample_provider: &'a SampleProvider) -> InstrumentUpdate<'a> {
	self.instrument.next_sample(sample_provider, self.freq)
    }
}

// --------------------------------------------------------------------------------

trait ChannelResampler {
    fn play(&mut self, sample_provider: &SampleProvider, dest: &mut [f32], channel: &mut ChannelState);
    // Called after a frequency change
    fn updated_frequency(&mut self, _channel: &mut ChannelState) {}
    fn updated_instrument(&mut self, _channel: &mut ChannelState) {}
}


// ================================================================================
// ChannelPlayer

#[derive(Clone)]
struct ChannelPlayer<T: ChannelResampler> {
    state: ChannelState,
    resampler: T,
}

impl<'a, T : ChannelResampler> ChannelPlayer<T> {
    fn new(resampler: T) -> Self {
	ChannelPlayer {
	    state: ChannelState::new(),
	    resampler,
	}
    }

    fn play(&mut self, sample_provider: &SampleProvider, dest: &mut [f32]) {
	if self.state.volume == 0.0 || self.state.freq == 0 {
	    return;
	}
	self.resampler.play(sample_provider, dest, &mut self.state);
    }


    /// Like play, but fade volume to zero at the end of dest.
    /// Does not update the channel volume.
    fn play_fadeout(&mut self, sample_provider: &SampleProvider, dest: &mut [f32]) {
	if dest.len() == 0 {
	    return;
	}
	let mut tmp = vec![0.0; dest.len()];
	self.play(sample_provider, &mut tmp);
	let volume_fraction = 1.0 / dest.len() as f32;
	let mut volume = 1.0;
	for i in 0..dest.len() {
	    let volume_weight = volume;// * volume;
	    volume -= volume_fraction;
	    dest[i] += tmp[i] * volume_weight;
	}
    }

    fn set_frequency(&mut self, freq: usize) {
	self.state.freq = freq;
	self.resampler.updated_frequency(&mut self.state);
    }

    fn set_instrument(&mut self, instr: Instrument) {
	self.state.instrument = instr.clone();
	self.resampler.updated_instrument(&mut self.state);
    }

    fn set_volume(&mut self, volume: f32) {
	self.state.volume = volume;
    }
}


// ================================================================================
// SinePlayer

struct SineResampler {
}

#[allow(unused)]
impl SineResampler {
    fn new() -> Self {
	SineResampler {
	}
    }
}

impl ChannelResampler for SineResampler {
    fn play(&mut self, _sample_provider: &SampleProvider, buf: &mut [f32], state: &mut ChannelState) {
	mk_sine(buf, state.freq);
    }
}

// ================================================================================
// SincResamplingPlayer

struct SincResampler {
    current_sample: Vec<f32>,
    current_resampled_sample: Vec<f32>,
    current_inpos: usize,
}

#[allow(unused)]
impl SincResampler {
    fn new() -> Self {
	SincResampler {
	    current_sample: vec![],
	    current_resampled_sample: vec![],
	    current_inpos: 0,
	}
    }

    fn resample(&mut self, channel: &ChannelState) {
	let relative_inpos = self.current_inpos as f64 / self.current_resampled_sample.len() as f64;

	let params = SincInterpolationParameters {
	    sinc_len: 2,
	    f_cutoff: 0.95,
	    interpolation: SincInterpolationType::Linear,
	    oversampling_factor: 16,
	    window: WindowFunction::Hann,
	};
	let resample_ratio = channel.resample_ratio();

	let mut resampler = SincFixedIn::<f32>::new(
	    resample_ratio,
	    1.0,
	    params,
	    self.current_sample.len(),
	    1,
	).unwrap();

	let waves_in = vec![&self.current_sample];
	let waves_out = resampler.process(&waves_in, None).unwrap();
	trace!("    #<resample># Converted {} samples at freq {} to length {}, ratio={resample_ratio}",
		 self.current_sample.len(),
		 channel.freq,
		 waves_out[0].len()
	);
	self.current_resampled_sample = waves_out[0].clone();
	self.current_inpos = (self.current_resampled_sample.len() as f64 * relative_inpos) as usize;
	// let pcm_resampled = &waves_out[0];
	// for x in 0 .. duration {
	//     let pos = start + x;
	//     let v = pcm_resampled[x % pcm_resampled.len()];
	//     buf[pos] = v;
	// }
    }
}

impl ChannelResampler for SincResampler {
    fn play(&mut self, sample_provider: &SampleProvider, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	while pos < buf.len() {
	    if self.current_inpos >= self.current_resampled_sample.len() {
		match channel.next_instrument_sample(sample_provider) {
		    InstrumentUpdate::None => {
			return;
		    },
		    InstrumentUpdate::Loop => {
			self.current_inpos = 0;
		    },
		    InstrumentUpdate::New(sample, is_sliding) => {
			self.current_sample = sample.to_vec();
			self.resample(channel);
			if !is_sliding || self.current_inpos >= self.current_sample.len() {
			    self.current_inpos = 0;
			}
		    },
		}
	    }
	    let copy_len = usize::min(buf.len() - pos,
				      self.current_resampled_sample.len() - self.current_inpos);
	    //buf[pos..pos+copy_len].copy_from_slice(&self.current_resampled_sample[self.current_inpos..self.current_inpos+copy_len]);
	    for x in 0..copy_len {
		buf[x + pos] += self.current_resampled_sample[x + self.current_inpos] * volume;
	    }
	    pos += copy_len;
	    self.current_inpos += copy_len;
	}
    }

    fn updated_frequency(&mut self, channel: &mut ChannelState) {
	self.resample(channel);
    }
}

// ================================================================================
// DirectFFTPlayer

struct DirectFFTResampler {
    current_freq_sample: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    current_resampled_sample: Vec<f32>,
    current_inpos: usize,
    planner: FftPlanner<f32>,
}

#[allow(unused)]
impl DirectFFTResampler {
    fn new() -> Self {
	DirectFFTResampler {
	    current_freq_sample: vec![],
	    scratch: vec![],
	    current_resampled_sample: vec![],
	    current_inpos: 0,
	    planner: FftPlanner::new(),
	}
    }

    fn resample(&mut self, channel: &ChannelState) {
	let relative_inpos = self.current_inpos as f64 / self.current_resampled_sample.len() as f64;

	let resample_ratio = channel.resample_ratio();

	let input_len = self.current_freq_sample.len();
	let output_len = (resample_ratio * input_len as f64) as usize;
	let fft = self.planner.plan_fft(output_len, FftDirection::Inverse);

	let mut complex_sample = if input_len < output_len {
	    let mut c = self.current_freq_sample.clone();
	    c.resize(output_len, Complex::new(0.0, 0.0));
	    c
	} else {
	    // input_len > output_len
	    self.current_freq_sample[0..output_len].to_vec()
	};
	self.ensure_scratch(fft.get_inplace_scratch_len());
	fft.process_with_scratch(&mut complex_sample,
				 &mut self.scratch);

	self.current_resampled_sample = complex_sample.iter().map(|&c| c.re).collect();
	self.current_inpos = (self.current_resampled_sample.len() as f64 * relative_inpos) as usize;
    }

    fn ensure_scratch(&mut self, len: usize) {
	if self.scratch.len() < len {
	    self.scratch = vec![Complex::new(0.0, 0.0); len];
	}
    }

    fn set_current_sample(&mut self, freqspace: &[f32]) {
	self.current_freq_sample = freqspace.iter().map(|&re| Complex::new(re, 0.0)).collect();
	let len =  self.current_freq_sample.len();
	let fft = self.planner.plan_fft(len, FftDirection::Forward);
	self.ensure_scratch(fft.get_inplace_scratch_len());
	fft.process_with_scratch(&mut self.current_freq_sample,
				 &mut self.scratch);
	self.filter_sample();
    }

    fn filter_sample(&mut self) {
	// let len = self.current_freq_sample.len();
	// let filter_start = len / 5;
	// self.current_freq_sample[filter_start..len].fill(Complex::new(0.0,0.0));
	self.butterworth_filter(3, 12500.0);
    }

    fn butterworth_filter(&mut self,
			  order: usize, cutoff_freq: f32) {
	let sample_rate = SAMPLE_RATE as f32;
	let nyquist = sample_rate / 2.0;
	let normalized_cutoff = cutoff_freq / nyquist;

	let len = self.current_freq_sample.len();
	for (i, freq_bin) in self.current_freq_sample.iter_mut().enumerate() {
            let frequency = (i as f32 / len as f32) * sample_rate;
            let normalized_frequency = frequency / nyquist;

            // Calculate the Butterworth response
            let response = 1.0 / (1.0 + (normalized_frequency / normalized_cutoff).powi(2 * order as i32) as f32).sqrt();

            // Apply the filter
            *freq_bin = *freq_bin * Complex::new(response, 0.0);
	}
    }
}

impl ChannelResampler for DirectFFTResampler {
    fn play(&mut self, sample_provider: &SampleProvider, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	while pos < buf.len() {
            if self.current_inpos >= self.current_resampled_sample.len() {
		match channel.next_instrument_sample(sample_provider) {
                    InstrumentUpdate::None => {
			return;
                    },
                    InstrumentUpdate::Loop => {
			self.current_inpos = 0;
                    },
                    InstrumentUpdate::New(sample, is_sliding) => {
			self.set_current_sample(&sample);
			self.resample(channel);
			if !is_sliding || self.current_inpos >= sample.len() {
			    self.current_inpos = 0;
			}
                    },
		}
            }
            let copy_len = usize::min(buf.len() - pos,
                                      self.current_resampled_sample.len() - self.current_inpos);
            for x in 0..copy_len {
		buf[x + pos] += self.current_resampled_sample[x + self.current_inpos] * volume;
            }
            pos += copy_len;
            self.current_inpos += copy_len;
        }
    }

    fn updated_frequency(&mut self, channel: &mut ChannelState) {
	self.resample(channel);
    }
}

// ================================================================================
// NearestResampler

struct NearestResampler {
    current_sample: Vec<f32>,
    current_inpos: f32,
}

#[allow(unused)]
impl NearestResampler {
    fn new() -> Self {
	NearestResampler {
	    current_sample: vec![],
	    current_inpos: 0.0,
	}
    }
}


impl ChannelResampler for NearestResampler {
    fn play(&mut self, sample_provider: &SampleProvider, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	let stride = channel.inv_resample_ratio() as f32;
	trace!(" stride={stride}");
	while pos < buf.len() {
	    let mut inpos = self.current_inpos as usize;
            if inpos >= self.current_sample.len() {
		match channel.next_instrument_sample(sample_provider) {
                    InstrumentUpdate::None => {
			return;
                    },
                    InstrumentUpdate::Loop => {
			self.current_inpos = 0.0;
			inpos = 0;
                    },
                    InstrumentUpdate::New(sample, is_sliding) => {
			self.current_sample = sample.to_vec();
			self.current_inpos = 0.0;
			if !is_sliding || inpos >= self.current_sample.len() {
			    inpos = 0;
			}
                    },
		}
            }
	    let v = self.current_sample[inpos];
	    //BLEPPER.apply_blep(buf, pos, v * volume);
	    buf[pos] += v * volume;
	    pos += 1;
            self.current_inpos += stride;
        }
    }
}

// ================================================================================
// LinearResampler

#[derive(Clone)]
struct LinearResampler {
    current_sample: Vec<f32>,
    current_inpos: f32,
    prev: f32,
}

#[allow(unused)]
impl LinearResampler {
    fn new() -> Self {
	LinearResampler {
	    current_sample: vec![],
	    current_inpos: 0.0,
	    prev: 0.0,
	}
    }

    fn ensure_sample(&mut self, sample_provider: &SampleProvider, channel: &mut ChannelState, inpos_delta: usize) -> Option<usize> {
	let inpos = self.current_inpos as usize + inpos_delta;
        if inpos >= self.current_sample.len() {
	    match channel.next_instrument_sample(sample_provider) {
                InstrumentUpdate::None => {
		    trace!("  <none>");
		    return None;
                },
                InstrumentUpdate::Loop => {
		    self.current_inpos -= self.current_sample.len() as f32;
		    trace!("  <loop>");
		    return Some(self.current_inpos as usize + inpos_delta);
                },
                InstrumentUpdate::New(sample, is_sliding) => {
		    trace!("  <new sample>");
		    if self.current_sample.len() > 0 {
			trace!("   -- Old:");
			for i in 0..10 {
			    trace!("      {i:8}: {:}", self.current_sample[i]);
			}
			for i in self.current_sample.len() - 10..self.current_sample.len() {
			    trace!("      {i:8}: {:}", self.current_sample[i]);
			}
		    }
		    self.current_sample = sample.to_vec();
		    if self.current_sample.len() > 0 {
			trace!("   -- New:");
			for i in 0..10 {
			    trace!("      {i:8}: {:}", self.current_sample[i]);
			}
		    }
		    if !is_sliding || self.current_inpos as usize >= self.current_sample.len() {
			self.current_inpos = 0.0;
		    }
		    return Some(inpos as usize);
                    },
		}
        }
	return Some(inpos);
    }
}


impl ChannelResampler for LinearResampler {
    fn play(&mut self, sample_provider: &SampleProvider, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	let stride = channel.inv_resample_ratio() as f32;
	trace!(" stride={stride}");
	while pos < buf.len() {
	    let inpos_i = match self.ensure_sample(sample_provider, channel, 0) {
		None => return,
		Some(v) => v,
	    };

	    let base_v = self.current_sample[inpos_i];
	    let inpos_frac = self.current_inpos.fract();
	    let v = base_v * inpos_frac + (self.prev * (1.0 - inpos_frac));
	    let next_inpos_f = self.current_inpos + stride;
	    self.current_inpos = next_inpos_f;
	    if next_inpos_f as usize > inpos_i {
		let last_sample = if self.current_sample.len() > 0 { self.current_sample[self.current_sample.len() - 1] } else { v };
		self.ensure_sample(sample_provider, channel, 0);
		self.prev = if self.current_sample.len() == 0 || self.current_inpos as usize == 0 {
		    last_sample
		} else { self.current_sample[self.current_inpos as usize - 1] };
	    }

	    // if (next_inpos_f as usize) == inpos_i {
	    // 	v = base_v;
	    // } else {
	    // 	let current_inpos_fraction = self.current_inpos.fract();
	    // 	v = base_v * (1.0 - current_inpos_fraction);

	    // 	let stride_len = (stride + current_inpos_fraction) as usize;
	    // 	let mut no_final = false;
	    // 	for i in 0..stride_len {
	    // 	    inpos_i = match self.ensure_sample(channel, i) {
	    // 		None => { no_final = true; break;},
	    // 		Some(v) => v,
	    // 	    };
	    // 	    v += self.current_sample[inpos_i];
	    // 	}
	    // 	let next_inpos_fraction = next_inpos_f.fract();
	    // 	if !no_final {
	    // 	    v += self.current_sample[inpos_i] * next_inpos_fraction;
	    // 	}
	    // 	v /= stride;
	    // }


	    if false {
		BLEPPER.apply_blep(buf, pos, v * volume);
	    } else {
		buf[pos] += v * volume;
	    }
	    pos += 1;
        }
    }

    fn updated_instrument(&mut self, _channel: &mut ChannelState) {
	if self.current_sample.len() > 0 {
	    self.current_inpos = 0.0;
	    self.prev = 0.0;
	}
	self.current_sample = vec![];
    }
}


// ================================================================================

type DefaultChannelPlayer = ChannelPlayer<LinearResampler>;

//fn mk_player() -> ChannelPlayer<SincResampler> {  ChannelPlayer::new(SincResampler::new()) }
//fn mk_player() -> ChannelPlayer<DirectFFTResampler> {  ChannelPlayer::new(DirectFFTResampler::new()) }
//fn mk_player() -> ChannelPlayer<NearestResampler> {  ChannelPlayer::new(NearestResampler::new()) }
fn mk_player() -> ChannelPlayer<LinearResampler> {  ChannelPlayer::new(LinearResampler::new()) }

#[derive(Clone)]
struct SingleSongPlayer {
    buf_pos_ms: [usize; 4],
    poly_it: SongIterator,
    new_instruments: [Option<Instrument>; 4],
    players: [DefaultChannelPlayer; 4],
    tick: usize,
}

impl SingleSongPlayer {

    fn new(poly_it: &SongIterator) -> Self {
	SingleSongPlayer {
	    buf_pos_ms: [0, 0, 0, 0],
	    poly_it: (*poly_it).clone(),
	    new_instruments: [None, None, None, None],
	    players:
	    [
		mk_player(),
		mk_player(),
		mk_player(),
		mk_player(),
	    ],
	    tick: 0,
	}
    }

    fn reset(&mut self) {
	self.buf_pos_ms = [0, 0, 0, 0];
	self.poly_it.reset();
	self.new_instruments = [None, None, None, None];
	self.players = [
	    mk_player(),
	    mk_player(),
	    mk_player(),
	    mk_player(),
	];
	self.tick = 0;
    }

    fn set_channel_logger(&mut self, channel: u8, logger: Arc<Mutex<dyn StreamLogger>>) {
	self.poly_it.channels[channel as usize].set_logger(logger);
    }

    fn fill(&mut self, sample_provider: &SampleProvider, channel: u8, buf: &mut [f32], sample_rate: usize) {
	debug!("SingleSongPlayer::fill({}, {sample_rate})", buf.len());
	let i = channel as usize;
	let mut d = VecDeque::<AQOp>::new();
	if self.poly_it.channels[i].is_done() {
	    self.poly_it.channels[channel as usize].logger.log("X", "D", format!("done"));
	    // FIXME: this should happen when ALL channels are done
	    // (though that should normally coincide.....)
	    self.poly_it.next_division();
	}
	self.poly_it.channels[i].next(&mut d);

	for dd in d {
	    trace!("  #{i}- {dd:?}");
	    match dd {
		AQOp::SetSamples(samples) => {
		    self.new_instruments[i] = Some(Instrument::new(samples));
		},
		AQOp::WaitMillis(ms) => {
		    let start = 0;
		    let stop = buf.len();
		    // let start = (sample_rate * self.buf_pos_ms[i]) / 1000;
		    self.buf_pos_ms[i] += ms;
		    // let stop = (sample_rate * self.buf_pos_ms[i]) / 1000;

		    if let Some(instr) = &self.new_instruments[i] {
			// Fade out old instrument
			// FIXME if we want to make this incremental: always want the same fade-out
			let fade_end = usize::min(stop, start + sample_rate / 4000);
			self.players[i].play_fadeout(&sample_provider, &mut buf[start..fade_end]);
			self.players[i].set_instrument(instr.clone());
			self.new_instruments[i] = None;
		    }
		    self.players[i].play(&sample_provider, &mut buf[start..stop]);
		},
		AQOp::SetVolume(v) => {
		    self.players[i].set_volume(v);
		},
		AQOp::SetFreq(f) => // freq = f / 32,
		    self.players[i].set_frequency(f), // FIXME: workaround for period_to_freq
		//AQOp::Timeslice => poly_it.adv,
		_ => {},
	    }
	}
    }
}

// ================================================================================
// ================================================================================

pub trait SongTracer : Sync + Send {
    /// Audio buffer for this tick
    fn trace_buf(&mut self, tick: usize, channel: u8, buf: Vec<f32>);
    fn change_song(&mut self) {}
    fn trace_message(&mut self, tick: usize, channel: u8, subsystem: &'static str, category: &'static str, msg: String);
    fn trace_message_num(&mut self, tick: usize, channel: u8, subsystem: &'static str, category: &'static str, msg: isize);
}

struct SongTracerStreamLogger {
    tracer: Arc<Mutex<dyn SongTracer>>,
    channel: u8,
    tick: usize,
}

impl SongTracerStreamLogger {
    fn new(tracer: Arc<Mutex<dyn SongTracer>>, channel: u8) -> Self {
	SongTracerStreamLogger {
	    tracer,
	    channel,
	    tick: 0,
	}
    }

    fn set_tick(&mut self, new_tick: usize) {
	self.tick = new_tick;
    }
}

impl StreamLogger for SongTracerStreamLogger {
    fn log(&mut self, subsystem : &'static str, category : &'static str, message : String) {
	let mut guard = self.tracer.lock().unwrap();
	guard.trace_message(self.tick, self.channel, subsystem, category, message);
    }
    fn log_num(&mut self, subsystem : &'static str, category : &'static str, message : isize) {
	let mut guard = self.tracer.lock().unwrap();
	guard.trace_message_num(self.tick, self.channel, subsystem, category, message);
    }
}

// ----------------------------------------

const BUF_SIZE : usize = 8000;

pub struct SongPlayer {
    song: Option<SingleSongPlayer>,
    tick: usize, // Song tick counter
    sample_provider: SampleProvider,
    left_buf: Vec<f32>,
    right_buf: Vec<f32>,
    tracer: Option<Arc<Mutex<dyn SongTracer>>>,
    stream_loggers: Vec<Arc<Mutex<SongTracerStreamLogger>>>,
    record: bool,
    writer: Option<WavWriter<BufWriter<File>>>,
}

impl SongPlayer {
    fn new(sample_data: &SampleData, songs: &[Song]) -> Self {
	// FIXME: use SampleProvider instead of SampleData
	let sample_provider = SampleProvider::new(sample_data, songs);
	// TODO: migrate to sample_provider as sample source for SongPlayer

	SongPlayer {
	    song: None,
	    tick: 0,
	    sample_provider,
	    left_buf: Vec::<f32>::with_capacity(BUF_SIZE),
	    right_buf: Vec::<f32>::with_capacity(BUF_SIZE),
	    tracer: None,
	    stream_loggers: Vec::new(),
	    record: false,
	    writer: None,
	}
    }

    fn start_recording(&mut self) {
	self.stop_recording();
	let spec = hound::WavSpec {
	    channels: 2,
	    sample_rate: 48000, // FIXME
	    bits_per_sample: 16,
	    sample_format: hound::SampleFormat::Int,
	};
	let writer = hound::WavWriter::create(format!("song.wav"), spec).unwrap();
	self.writer = Some(writer);
    }

    fn stop_recording(&mut self) {
	let w = mem::replace(&mut self.writer, None);
	w.map(|x| x.finalize().unwrap());
    }

    fn stop(&mut self) {
	self.song = None;
	self.left_buf.clear();
	self.right_buf.clear();
	self.report_change_song();
	self.stop_recording();
    }

    fn play(&mut self, song_it: &SongIterator) {
	self.song = Some(SingleSongPlayer::new(song_it));
	self.tick = 0;
	self.report_change_song();
	self.update_channel_loggers();
	if let Some(ref mut song) = self.song {
	    song.reset();
	}
	if self.record {
	    self.start_recording();
	}
    }

    pub fn update_channel_loggers(&mut self) {
	if let Some(ref mut song) = self.song {
	    if self.stream_loggers.len() == 4 {
		for n in 0..4 {
		    song.set_channel_logger(n, self.stream_loggers[n as usize].clone());
		}
	    } else {
		for n in 0..4 {
		    song.set_channel_logger(n, streamlog::dummy());
		}
	    }
	}
    }

    fn channel_loggers_update_tick(&mut self) {
	for c in self.stream_loggers.iter_mut() {
	    let mut guard = c.lock().unwrap();
	    guard.set_tick(self.tick);
	}
    }

    pub fn set_tracer(&mut self, tracer: Arc<Mutex<dyn SongTracer>>) {
	self.tracer = Some(tracer.clone());
	for c in 0..4 {
	    let logger = Arc::new(Mutex::new(SongTracerStreamLogger::new(tracer.clone(), c)));
	    self.stream_loggers.push(logger);
	}
	self.update_channel_loggers();
    }

    pub fn clear_tracer(&mut self) {
	self.tracer = None;
	self.stream_loggers.clear();
	self.update_channel_loggers();
    }

    fn have_tracer(&self) -> bool {
	self.tracer.is_some()
    }

    fn report_buf(&self, tick: usize, channel: u8, buf: &[f32]) {
	match &self.tracer {
	    None => {},
	    Some(tracer) => {
		let mut guard = tracer.lock().unwrap();
		guard.trace_buf(tick, channel, buf.to_vec());
	    }
	}
    }

    fn report_change_song(&self) {
	match &self.tracer {
	    None => {},
	    Some(tracer) => {
		let mut guard = tracer.lock().unwrap();
		guard.change_song();
	    }
	}
    }

    // buf_left and buf_right are guaranteed to have exactly one tick in length
    fn fill_channels(&mut self, buf_left: &mut [f32], buf_right: &mut [f32], sample_rate: usize) {
	let mut to_report = vec![];
	let trace = self.have_tracer();

	if let Some(ref mut sp) = self.song {
	    let channels = [0, 1, 1, 0];

	    for i in 0..4 {
		let buf = if channels[i] == 1 { &mut* buf_right } else { &mut* buf_left };

		if trace {
		    let mut data: Vec<f32> = vec![0.0; buf.len()];
		    sp.fill(&self.sample_provider,
			    i as u8,
			    &mut data,
			    sample_rate);
		    for (d, &s) in buf.iter_mut().zip(data.iter()) {
			*d += s;
		    }
		    to_report.push(data);
		} else {
		    sp.fill(&self.sample_provider,
			    i as u8,
			    buf,
			    sample_rate);
		}
	    }
	}
	if trace {
	    for (i, buf) in to_report.iter().enumerate() {
		self.report_buf(self.tick, i as u8, buf);
	    }
	}

	if self.song.is_some() {
	    self.tick += 1;
	    self.channel_loggers_update_tick();
	}
    }

    fn fill(&mut self, buf_left: &mut [f32], buf_right: &mut [f32], sample_rate: usize) {
	info!("SongPlayer::fill({}, {}, {sample_rate})", buf_left.len(), buf_right.len());
	let samples_per_tick = sample_rate / 50;
	let mut pos = 0;
	if self.left_buf.len() > 0 {
	    let leftover_length = self.left_buf.len();
	    assert!(leftover_length < buf_left.len());
	    buf_left[0..leftover_length].copy_from_slice(&self.left_buf);
	    buf_right[0..leftover_length].copy_from_slice(&self.right_buf);

	    pos += leftover_length;
	    self.left_buf.clear();
	    self.right_buf.clear();

	}
	if let Some(ref mut _sp) = self.song {
	    while pos + samples_per_tick <= buf_left.len() {
		debug!("  pos={pos}, += {samples_per_tick}");
		let end = pos + samples_per_tick;
		self.fill_channels(&mut buf_left[pos..end],
				   &mut buf_right[pos..end],
				   sample_rate);

		pos += samples_per_tick
	    }
	    if pos < buf_left.len() {
		let remaining = buf_left.len() - pos;
		let mut local_left = [0.0; BUF_SIZE];
		let mut local_right = [0.0; BUF_SIZE];
		debug!("  special handler: pos={pos}, left = {}, remain={remaining}", buf_left.len());
		// Partial write
		self.fill_channels(&mut local_left[0..samples_per_tick],
				   &mut local_right[0..samples_per_tick],
				   sample_rate);

		buf_left[pos..].copy_from_slice(&local_left[0..remaining]);
		buf_right[pos..].copy_from_slice(&local_right[0..remaining]);
		self.left_buf.extend_from_slice(&local_left[remaining..samples_per_tick]);
		self.right_buf.extend_from_slice(&local_right[remaining..samples_per_tick]);
	    }

	    if let Some(ref mut writer) = self.writer {
		for t in 0..buf_left.len() {
		    writer.write_sample((buf_left[t] * 32767.0) as i16).unwrap();
		    writer.write_sample((buf_right[t] * 32767.0) as i16).unwrap();
		}
	    }
	}
    }
}

impl AudioSource for SongPlayer {
    fn fill(&mut self, buf_left: &mut [f32], buf_right: &mut [f32], sample_rate: usize) -> usize {
	// let mut guard = self.player.lock().unwrap();
	self.fill(buf_left, buf_right, sample_rate);
	buf_left.len()
    }
}

pub struct SongPlayerAudioSource {
    player: Arc<Mutex<SongPlayer>>,
}

impl SongPlayerAudioSource {
    pub fn new(sample_data: &SampleData, songs: &[Song]) -> Self {
	SongPlayerAudioSource {
	    player: Arc::new(Mutex::new(SongPlayer::new(sample_data, songs)))
	}
    }

    pub fn set_tracer(&mut self, tracer: Arc<Mutex<dyn SongTracer>>) {
	let mut guard = self.player.lock().unwrap();
	guard.set_tracer(tracer);
    }

    pub fn clear_tracer(&mut self) {
	let mut guard = self.player.lock().unwrap();
	guard.clear_tracer();
    }

    pub fn stop(&mut self) {
	let mut guard = self.player.lock().unwrap();
	guard.stop();
    }

    pub fn play(&mut self, poly_it: &SongIterator) {
	let mut guard = self.player.lock().unwrap();
	guard.play(poly_it);
    }

    pub fn player(&self) -> Arc<Mutex<SongPlayer>> {
	self.player.clone()
    }
}

