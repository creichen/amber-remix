
use std::collections::VecDeque;

use hound::{WavWriter, WavSpec, SampleFormat};
#[allow(unused)]
use lazy_static::lazy_static;
use log::{Level, log_enabled, trace, debug, info, warn, error};
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use rustfft::{FftPlanner, num_complex::Complex, FftDirection};
//use sdl2::libc::STA_FREQHOLD;
use crate::{datafiles::{music::Song, sampledata::SampleData}, audio::{AQOp, AudioIterator}};
use super::{amber::SongIterator, AQSample, SampleRange};
use super::blep::BLEP;

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
    sample_data: Option<&'a SampleData>,
    ops: Vec<AQSample>,
    last_range: Option<SampleRange>,
}

impl<'a> Instrument<'a> {
    fn new(sample_data: &'a SampleData,
	   ops: Vec<AQSample>) -> Self {

	// println!("WRITING");
	// let pcm_data = &sample_data[0xd7b2..0xe0b8];
	//     let spec = WavSpec {
	// 	channels: 1,
	// 	sample_rate: 20000,
	// 	bits_per_sample: 16,
	// 	sample_format: SampleFormat::Int,
	//     };
	// println!("foo");
	// let mut writer = WavWriter::create("instr.wav", spec).unwrap();
	// println!("bar");
	// for &sample in pcm_data {
        //     writer.write_sample(sample as i16 * 256).unwrap();
	// }
	// println!("quux");
	// writer.finalize().unwrap();
	Instrument {
	    ops,
	    sample_data: Some(sample_data),
	    last_range: None,
	}
    }

    fn empty() -> Self {
	Instrument {
	    sample_data: None,
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
	    Some(r) => match self.sample_data {
		None => None,
		Some(d) => Some(&d[r]),
	    }
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
	    println!("    -> looping sample: {:?}", self.current_sample_range());
	    return InstrumentUpdate::Loop;
	}
	// otherwise we have an actual update
	let sample = self.current_sample();
	println!("    -> single sample: {:?}", self.current_sample_range());
	if !self.is_looping() {
	    self.ops = self.ops[1..self.ops.len()].to_vec();
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

lazy_static! {
    static ref BLEPPER: BLEP = BLEP::new();
}

// ================================================================================
struct ChannelState<'a> {
    freq : usize,
    volume: f32,
    instrument: Instrument<'a>,
}

impl<'a> ChannelState<'a> {
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
}

// --------------------------------------------------------------------------------

trait ChannelResampler<'a> {
    fn play(&mut self, dest: &mut [f32], channel: &mut ChannelState<'a>);
    // Called after a frequency change
    fn updated_frequency(&mut self, channel: &mut ChannelState<'a>) {}
    fn updated_instrument(&mut self, channel: &mut ChannelState<'a>) {}
}


// ================================================================================
// ChannelPlayer
struct ChannelPlayer<'a, T: ChannelResampler<'a>> {
    state: ChannelState<'a>,
    resampler: T,
}

impl<'a, T : ChannelResampler<'a>> ChannelPlayer<'a, T> {
    fn new(resampler: T) -> Self {
	ChannelPlayer {
	    state: ChannelState::new(),
	    resampler,
	}
    }

    fn play(&mut self, dest: &mut [f32]) {
	if self.state.volume == 0.0 || self.state.freq == 0 {
	    return;
	}
	self.resampler.play(dest, &mut self.state);
    }


    /// Like play, but fade volume to zero at the end of dest.
    /// Does not update the channel volume.
    fn play_fadeout(&mut self, dest: &mut [f32]) {
	if dest.len() == 0 {
	    return;
	}
	let mut tmp = vec![0.0; dest.len()];
	self.play(&mut tmp);
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

    fn set_instrument(&mut self, instr: Instrument<'a>) {
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

impl SineResampler {
    fn new() -> Self {
	SineResampler {
	}
    }
}

impl<'a> ChannelResampler<'a> for SineResampler {
    fn play(&mut self, buf: &mut [f32], state: &mut ChannelState) {
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
	println!("    #<resample># Converted {} samples at freq {} to length {}, ratio={resample_ratio}",
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

impl<'a> ChannelResampler<'a> for SincResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	while pos < buf.len() {
	    if self.current_inpos >= self.current_resampled_sample.len() {
		match channel.instrument.next_sample() {
		    InstrumentUpdate::None => {
			return;
		    },
		    InstrumentUpdate::Loop => {
			self.current_inpos = 0;
		    },
		    InstrumentUpdate::New(sample) => {
			self.current_sample = sample;
			self.resample(channel);
			self.current_inpos = 0;
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

impl<'a> ChannelResampler<'a> for DirectFFTResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	while pos < buf.len() {
            if self.current_inpos >= self.current_resampled_sample.len() {
		match channel.instrument.next_sample() {
                    InstrumentUpdate::None => {
			return;
                    },
                    InstrumentUpdate::Loop => {
			self.current_inpos = 0;
                    },
                    InstrumentUpdate::New(sample) => {
			self.set_current_sample(&sample);
			self.resample(channel);
			self.current_inpos = 0;
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

impl NearestResampler {
    fn new() -> Self {
	NearestResampler {
	    current_sample: vec![],
	    current_inpos: 0.0,
	}
    }
}


impl<'a> ChannelResampler<'a> for NearestResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	let stride = channel.inv_resample_ratio() as f32;
	println!(" stride={stride}");
	while pos < buf.len() {
	    let mut inpos = self.current_inpos as usize;
            if inpos >= self.current_sample.len() {
		match channel.instrument.next_sample() {
                    InstrumentUpdate::None => {
			return;
                    },
                    InstrumentUpdate::Loop => {
			self.current_inpos = 0.0;
			inpos = 0;
                    },
                    InstrumentUpdate::New(sample) => {
			self.current_sample = sample;
			self.current_inpos = 0.0;
			inpos = 0;
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

struct LinearResampler {
    current_sample: Vec<f32>,
    current_inpos: f32,
    prev: f32,
}

impl LinearResampler {
    fn new() -> Self {
	LinearResampler {
	    current_sample: vec![],
	    current_inpos: 0.0,
	    prev: 0.0,
	}
    }

    fn ensure_sample(&mut self, channel: &mut ChannelState, inpos_delta: usize) -> Option<usize> {
	let inpos = self.current_inpos as usize + inpos_delta;
        if inpos >= self.current_sample.len() {
	    match channel.instrument.next_sample() {
                InstrumentUpdate::None => {
		    println!("  <none>");
		    return None;
                },
                InstrumentUpdate::Loop => {
		    self.current_inpos -= self.current_sample.len() as f32;
		    println!("  <loop>");
		    return Some(self.current_inpos as usize + inpos_delta);
                },
                InstrumentUpdate::New(sample) => {
		    println!("  <new sample>");
		    if self.current_sample.len() > 0 {
			println!("   -- Old:");
			for i in 0..10 {
			    println!("      {i:8}: {:}", self.current_sample[i]);
			}
			for i in self.current_sample.len() - 10..self.current_sample.len() {
			    println!("      {i:8}: {:}", self.current_sample[i]);
			}
		    }
		    self.current_sample = sample;
		    if self.current_sample.len() > 0 {
			println!("   -- New:");
			for i in 0..10 {
			    println!("      {i:8}: {:}", self.current_sample[i]);
			}
		    }
		    self.current_inpos = 0.0;
		    return Some(0);
                    },
		}
        }
	return Some(inpos);
    }
}


impl<'a> ChannelResampler<'a> for LinearResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	let stride = channel.inv_resample_ratio() as f32;
	println!(" stride={stride}");
	while pos < buf.len() {
	    let inpos_i = match self.ensure_sample(channel, 0) {
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
		self.ensure_sample(channel, 0);
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

    fn updated_instrument(&mut self, _channel: &mut ChannelState<'a>) {
	if self.current_sample.len() > 0 {
	    self.current_inpos = 0.0;
	    self.prev = 0.0;
	}
	self.current_sample = vec![];
    }
}


// ================================================================================

//fn mk_player<'a>() -> ChannelPlayer<'a, SincResampler> {  ChannelPlayer::new(SincResampler::new()) }
//fn mk_player<'a>() -> ChannelPlayer<'a, DirectFFTResampler> {  ChannelPlayer::new(DirectFFTResampler::new()) }
//fn mk_player<'a>() -> ChannelPlayer<'a, NearestResampler> {  ChannelPlayer::new(NearestResampler::new()) }
fn mk_player<'a>() -> ChannelPlayer<'a, LinearResampler> {  ChannelPlayer::new(LinearResampler::new()) }

/// Iterate over the song's poly iterator until the buffer is full
pub fn song_to_pcm(sample_data: &SampleData,
		   buf_left: &mut [f32],
		   buf_right: &mut [f32],
		   channel_mask: usize,
		   start_at_tick: usize,
		   song: &Song,
		   sample_rate: usize) {
    let mut poly_it = SongIterator::new(&song,
					song.songinfo.first_division,
					song.songinfo.last_division);

    let max_pos = buf_left.len();
    let duration_milliseconds = (max_pos * 1000) / sample_rate;
    let mut buf_pos_ms = [0, 0, 0, 0];
    let mut players = [
	mk_player(),
	mk_player(),
	mk_player(),
	mk_player(),
    ];

    let mut new_instruments: [Option<Instrument>; 4] = [
	None, None, None, None,
    ];

    let mut channels = [0, 1, 1, 0];
    for i in 0..4 {
	if channel_mask & (1 << i) == 0 {
	    channels[i] = -1;
	}
    }

    let mut tick = 0;

    // FIXME: doesn't necessarily iterate until buffer is full
    while buf_pos_ms[0] < duration_milliseconds
	&& buf_pos_ms[1] < duration_milliseconds
	&& buf_pos_ms[2] < duration_milliseconds
	&& buf_pos_ms[3] < duration_milliseconds {

	    println!("--- tick {buf_pos_ms:?} {tick} >= {start_at_tick}\n");

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
			    new_instruments[i] = Some(Instrument::new(sample_data, samples));
			},
			AQOp::WaitMillis(ms) => {
			    let start = (SAMPLE_RATE * buf_pos_ms[i]) / 1000;
			    if tick >= start_at_tick {
				buf_pos_ms[i] += ms;
			    }
			    let mut stop = (SAMPLE_RATE * buf_pos_ms[i]) / 1000;
			    if stop > max_pos {
				stop = max_pos;
			    }

			    if let Some(instr) = &new_instruments[i] {
				// Fade out old instrument
				// FIXME if we want to make this incremental: always want the same fade-out
				let fade_end = usize::min(stop, start + SAMPLE_RATE / 4000);
				if tick >= start_at_tick {
				    players[i].play_fadeout(&mut buf[start..fade_end]);
				}
				players[i].set_instrument(instr.clone());
				new_instruments[i] = None;
			    }
			    if tick >= start_at_tick {
				players[i].play(&mut buf[start..stop]);
			    }
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
	}
	    tick += 1;
    }
}
