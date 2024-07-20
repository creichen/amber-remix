
#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use rustfft::{FftPlanner, num_complex::Complex, FftDirection};
use std::collections::VecDeque;
use crate::{datafiles::{music::Song, sampledata::SampleData}, audio::{AQOp, AudioIterator}};
use super::{amber::SongIterator, AQSample, SampleRange};

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
// Instruments
#[derive(Clone, Debug)]
enum InstrumentUpdate {
    New(Vec<f32>),
    Loop,
    None
}

#[derive(Clone)]
struct Instrument<'a> {
    sample_data: &'a SampleData,
    ops: Vec<AQSample>,
    last_range: Option<SampleRange>,
}

impl<'a> Instrument<'a> {
    fn new(sample_data: &'a SampleData,
	   ops: Vec<AQSample>) -> Self {
	Instrument {
	    ops,
	    sample_data,
	    last_range: None,
	}
    }

    fn is_looping(&self) -> bool {
	match self.ops.as_slice() {
	    [AQSample::Loop(_), ..] => true,
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

    fn current_sample_raw(&self) -> Option<&'a [i8]> {
	match self.current_sample_range() {
	    None => None,
	    Some(r) => Some(&self.sample_data[r]),
	}
    }

    fn current_sample(&self) -> Vec<f32> {
	match self.current_sample_raw() {
	    None => vec![],
	    Some(pcm) => pcm.iter()
		.map(|&v| v as f32 / 128.0).collect(),
	}
    }

    fn next_sample(&mut self) -> InstrumentUpdate {
	let new_range = self.current_sample_range();
	if new_range.is_none() {
	    return InstrumentUpdate::None;
	}
	if new_range == self.last_range {
	    print!("    -> looping sample: {:?}", self.current_sample_range());
	    return InstrumentUpdate::Loop;
	}
	// otherwise we have an actual update
	let sample = self.current_sample();
	print!("    -> single sample: {:?}", self.current_sample_range());
	if !self.is_looping() {
	    self.ops.pop();
	}
	return InstrumentUpdate::New(sample);
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

// ================================================================================

trait ChannelPlayer<'a> {
    fn play(&mut self, dest: &mut [f32]);
    fn set_frequency(&mut self, freq: usize);
    fn set_instrument(&mut self, instr: Instrument<'a>);
    fn set_volume(&mut self, volume: f32);
}

// ================================================================================
// SinePlayer

struct SinePlayer {
    freq : usize,
}

impl SinePlayer {
    fn new() -> Self {
	SinePlayer {
	    freq: 1
	}
    }
}

impl<'a> ChannelPlayer<'a> for SinePlayer {
    fn play(&mut self, buf: &mut [f32]) {
	mk_sine(buf, self.freq);
    }

    fn set_frequency(&mut self, freq: usize) {
	self.freq = freq / 32; // Really depends on the instrument
    }

    fn set_instrument(&mut self, _instr: Instrument) {
    }

    fn set_volume(&mut self, _vol: f32) {
    }
}

// ================================================================================
// SincResamplingPlayer

struct SincResamplingPlayer<'a> {
    freq: usize,
    volume: f32,
    current_sample: Vec<f32>,
    current_resampled_sample: Vec<f32>,
    current_outpos: usize,
    instrument: Option<Instrument<'a>>,
}

impl<'a> SincResamplingPlayer<'a> {
    fn new() -> Self {
	SincResamplingPlayer {
	    volume: 0.0,
	    freq: 0,
	    current_sample: vec![],
	    current_resampled_sample: vec![],
	    current_outpos: 0,
	    instrument: None,
	}
    }

    fn resample(&mut self) {
	let relative_outpos = self.current_outpos as f64 / self.current_resampled_sample.len() as f64;

	let params = SincInterpolationParameters {
	    sinc_len: 32,
	    f_cutoff: 0.95,
	    interpolation: SincInterpolationType::Linear,
	    oversampling_factor: 16,
	    window: WindowFunction::BlackmanHarris2,
	};
	let sample_rate = SAMPLE_RATE as f64;
	let frequency = self.freq as f64;
	//let pcm_len = self.current_sample.len() as f64;

	//let resample_ratio = sample_rate / (pcm_len * frequency);
	let resample_ratio = 1.0 / (frequency / sample_rate);

	let mut resampler = SincFixedIn::<f32>::new(
	    resample_ratio,
	    1.0,
	    params,
	    self.current_sample.len(),
	    1,
	).unwrap();

	let waves_in = vec![&self.current_sample];
	let waves_out = resampler.process(&waves_in, None).unwrap();
	println!("    #<resample># Converted {} samples at freq {} to length {}, ratio={resample_ratio}",
		 self.current_sample.len(),
		 self.freq,
		 waves_out[0].len()
	);
	self.current_resampled_sample = waves_out[0].clone();
	self.current_outpos = (self.current_resampled_sample.len() as f64 * relative_outpos) as usize;
	// let pcm_resampled = &waves_out[0];
	// for x in 0 .. duration {
	//     let pos = start + x;
	//     let v = pcm_resampled[x % pcm_resampled.len()];
	//     buf[pos] = v;
	// }
    }
}

impl<'a> ChannelPlayer<'a> for SincResamplingPlayer<'a> {
    fn play(&mut self, buf: &mut [f32]) {
	let volume = self.volume;
	if self.freq == 0 || volume == 0.0 {
	    return;
	}

	let mut pos = 0;
	while pos < buf.len() {
	    if self.current_outpos >= self.current_resampled_sample.len() {
		if let Some(ref mut instr) = self.instrument {
		    match instr.next_sample() {
			InstrumentUpdate::None => {
			    self.instrument = None;
			    return;
			},
			InstrumentUpdate::Loop => {
			    self.current_outpos = 0;
			},
			InstrumentUpdate::New(sample) => {
			    self.current_sample = sample;
			    self.resample();
			    self.current_outpos = 0;
			},
		    }
		} else {
		    return;
		}
	    }
	    let copy_len = usize::min(buf.len() - pos,
				      self.current_resampled_sample.len() - self.current_outpos);
	    //buf[pos..pos+copy_len].copy_from_slice(&self.current_resampled_sample[self.current_outpos..self.current_outpos+copy_len]);
	    for x in 0..copy_len {
		buf[x + pos] += self.current_resampled_sample[x + self.current_outpos] * volume;
	    }
	    pos += copy_len;
	    self.current_outpos += copy_len;
	}
    }

    fn set_frequency(&mut self, freq: usize) {
	self.freq = freq;
	self.resample();
    }

    fn set_instrument(&mut self, instr: Instrument<'a>) {
	self.instrument = Some(instr.clone());
    }

    fn set_volume(&mut self, volume: f32) {
	self.volume = volume;
    }
}

// ================================================================================
// DirectFFTPlayer

struct DirectFFTPlayer<'a> {
    freq: usize,
    volume: f32,
    current_freq_sample: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    current_resampled_sample: Vec<f32>,
    current_outpos: usize,
    instrument: Option<Instrument<'a>>,
    planner: FftPlanner<f32>,
}

impl<'a> DirectFFTPlayer<'a> {
    fn new() -> Self {
	DirectFFTPlayer {
	    volume: 0.0,
	    freq: 0,
	    current_freq_sample: vec![],
	    scratch: vec![],
	    current_resampled_sample: vec![],
	    current_outpos: 0,
	    instrument: None,
	    planner: FftPlanner::new(),
	}
    }

    fn resample(&mut self) {
	let relative_outpos = self.current_outpos as f64 / self.current_resampled_sample.len() as f64;

	let sample_rate = SAMPLE_RATE as f64;
	let frequency = self.freq as f64;
	let resample_ratio = 1.0 / (frequency / sample_rate);

	let input_len = self.current_freq_sample.len();
	let output_len = (resample_ratio * input_len as f64) as usize;
	let fft = self.planner.plan_fft(output_len, FftDirection::Inverse);

	println!("input_len={input_len}, output_len={output_len}, scratchsize={}", self.scratch.len());
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
	self.current_outpos = (self.current_resampled_sample.len() as f64 * relative_outpos) as usize;
    }

    fn ensure_scratch(&mut self, len: usize) {
	println!("  Ensuring scratch space: is {}, needed {len}", self.scratch.len());
	if self.scratch.len() < len {
	    println!("   -> resized");
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
	let len = self.current_freq_sample.len();
	let filter_start = len / 5;
	self.current_freq_sample[filter_start..len].fill(Complex::new(0.0,0.0));
    }
}

impl<'a> ChannelPlayer<'a> for DirectFFTPlayer<'a> {
    fn play(&mut self, buf: &mut [f32]) {
	let volume = self.volume;
	if self.freq == 0 || volume == 0.0 {
	    return;
	}

	let mut pos = 0;
	while pos < buf.len() {
	    if self.current_outpos >= self.current_resampled_sample.len() {
		if let Some(ref mut instr) = self.instrument {
		    match instr.next_sample() {
			InstrumentUpdate::None => {
			    self.instrument = None;
			    return;
			},
			InstrumentUpdate::Loop => {
			    self.current_outpos = 0;
			},
			InstrumentUpdate::New(sample) => {
			    self.set_current_sample(&sample);
			    self.resample();
			    self.current_outpos = 0;
			},
		    }
		} else {
		    return;
		}
	    }
	    let copy_len = usize::min(buf.len() - pos,
				      self.current_resampled_sample.len() - self.current_outpos);
	    for x in 0..copy_len {
		buf[x + pos] += self.current_resampled_sample[x + self.current_outpos] * volume;
	    }
	    pos += copy_len;
	    self.current_outpos += copy_len;
	}
    }

    fn set_frequency(&mut self, freq: usize) {
	self.freq = freq;
	self.resample();
    }

    fn set_instrument(&mut self, instr: Instrument<'a>) {
	self.instrument = Some(instr.clone());
    }

    fn set_volume(&mut self, volume: f32) {
	self.volume = volume;
    }
}


// ================================================================================

/// Iterate over the song's poly iterator until the buffer is full
pub fn song_to_pcm(sample_data: &SampleData,
		   buf_left: &mut [f32],
		   buf_right: &mut [f32],
		   song: &Song,
		   sample_rate: usize) {
    let mut poly_it = SongIterator::new(&song,
					song.songinfo.first_division,
					song.songinfo.last_division);

    let max_pos = buf_left.len();
    let duration_milliseconds = (max_pos * 1000) / sample_rate;
    let mut buf_pos_ms = [0, 0, 0, 0];
    let mut players = [
	SincResamplingPlayer::new(),
	SincResamplingPlayer::new(),
	SincResamplingPlayer::new(),
	SincResamplingPlayer::new(), ];
    // let mut players = [
    // 	DirectFFTPlayer::new(),
    // 	DirectFFTPlayer::new(),
    // 	DirectFFTPlayer::new(),
    // 	DirectFFTPlayer::new(), ];

    let channels = [0, 1, 1, 0];

    // FIXME: doesn't necessarily iterate until buffer is full
    while buf_pos_ms[0] < duration_milliseconds {
//	let mut d2 = VecDeque::<AQOp>::new();

	println!("--- tick {buf_pos_ms:?}\n");

	for i in 0..4 {
	    let mut d = VecDeque::<AQOp>::new();
	    let out_channel = channels[i];
	    if out_channel < 0 {
		// suppress
		poly_it.channels[i].next(&mut d);
	    } else {
		poly_it.channels[i].next(&mut d);
		if poly_it.channels[i].is_done() {
		    // FIXME: this should happen when ALL channels are done
		    // (though that should normally coincide.....)
		    poly_it.next_division();
		}

		let buf = if out_channel == 0
		    {&mut*buf_left } else { &mut*buf_right };

		for dd in d {
		    println!("  #{i}- {dd:?}");
		    match dd {
			AQOp::SetSamples(samples) => {
			    players[i].set_instrument(Instrument::new(sample_data,
								      samples));
			},
			AQOp::WaitMillis(ms) => {
			    let start = (SAMPLE_RATE * buf_pos_ms[i]) / 1000;
			    buf_pos_ms[i] += ms;
			    let mut stop = (SAMPLE_RATE * buf_pos_ms[i]) / 1000;
			    if stop > max_pos {
				stop = max_pos;
			    }
			    players[i].play(&mut buf[start..stop]);
			    // mk_audio(&sample_data,
			    // 	     buf,
			    // 	     &mut current_instrument,
			    // 	     &mut last_instr_sample,
			    // 	     start,
			    // 	     stop - start,
			    // 	     freq);
			},
			AQOp::SetVolume(v) => {
			    players[i].set_volume(v);
			},
			AQOp::SetFreq(f) => // freq = f / 32,
			    players[i].set_frequency(f), // FIXME: workaround for period_to_freq
			//AQOp::Timeslice => poly_it.adv,
			_ => {},
		    }
		}
	    }
	    //println!(" {dd:?}\n");
	}
    }
}
