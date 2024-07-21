
#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
use rustfft::{FftPlanner, num_complex::Complex, FftDirection};
//use sdl2::libc::STA_FREQHOLD;
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
    sample_data: Option<&'a SampleData>,
    ops: Vec<AQSample>,
    last_range: Option<SampleRange>,
}

impl<'a> Instrument<'a> {
    fn new(sample_data: &'a SampleData,
	   ops: Vec<AQSample>) -> Self {
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
	    //println!("    -> looping sample: {:?}", self.current_sample_range());
	    return InstrumentUpdate::Loop;
	}
	// otherwise we have an actual update
	let sample = self.current_sample();
	//println!("    -> single sample: {:?}", self.current_sample_range());
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
    current_outpos: usize,
}

impl SincResampler {
    fn new() -> Self {
	SincResampler {
	    current_sample: vec![],
	    current_resampled_sample: vec![],
	    current_outpos: 0,
	}
    }

    fn resample(&mut self, channel: &ChannelState) {
	let relative_outpos = self.current_outpos as f64 / self.current_resampled_sample.len() as f64;

	let params = SincInterpolationParameters {
	    sinc_len: 2,
	    f_cutoff: 0.95,
	    interpolation: SincInterpolationType::Linear,
	    oversampling_factor: 16,
	    window: WindowFunction::Hann,
	};
	let sample_rate = SAMPLE_RATE as f64;
	let frequency = channel.freq as f64;
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
		 channel.freq,
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

impl<'a> ChannelResampler<'a> for SincResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	let mut pos = 0;
	while pos < buf.len() {
	    if self.current_outpos >= self.current_resampled_sample.len() {
		match channel.instrument.next_sample() {
		    InstrumentUpdate::None => {
			return;
		    },
		    InstrumentUpdate::Loop => {
			self.current_outpos = 0;
		    },
		    InstrumentUpdate::New(sample) => {
			self.current_sample = sample;
			self.resample(channel);
			self.current_outpos = 0;
		    },
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
    current_outpos: usize,
    planner: FftPlanner<f32>,
}

impl DirectFFTResampler {
    fn new() -> Self {
	DirectFFTResampler {
	    current_freq_sample: vec![],
	    scratch: vec![],
	    current_resampled_sample: vec![],
	    current_outpos: 0,
	    planner: FftPlanner::new(),
	}
    }

    fn resample(&mut self, channel: &ChannelState) {
	let relative_outpos = self.current_outpos as f64 / self.current_resampled_sample.len() as f64;

	let sample_rate = SAMPLE_RATE as f64;
	let frequency = channel.freq as f64;
	let resample_ratio = 1.0 / (frequency / sample_rate);

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
	self.current_outpos = (self.current_resampled_sample.len() as f64 * relative_outpos) as usize;
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
           if self.current_outpos >= self.current_resampled_sample.len() {
               match channel.instrument.next_sample() {
                   InstrumentUpdate::None => {
                       return;
                   },
                   InstrumentUpdate::Loop => {
                       self.current_outpos = 0;
                   },
                   InstrumentUpdate::New(sample) => {
                       self.set_current_sample(&sample);
                       self.resample(channel);
                       self.current_outpos = 0;
                   },
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

    fn updated_frequency(&mut self, channel: &mut ChannelState) {
	self.resample(channel);
    }
}

// ================================================================================
// DirectFFTPlayer

struct LinearResampler {
    current_sample: Vec<f32>,
    current_outpos: f32,
}

impl LinearResampler {
    fn new() -> Self {
	LinearResampler {
	    current_sample: vec![],
	    current_outpos: 0.0,
	}
    }
}


impl<'a> ChannelResampler<'a> for LinearResampler {
    fn play(&mut self, buf: &mut [f32], channel: &mut ChannelState) {
	let volume = channel.volume;
	if channel.freq == 0 || volume == 0.0 {
	    return;
	}
	
    }
}


// ================================================================================

//fn mk_player<'a>() -> ChannelPlayer<'a, SincResampler> {  ChannelPlayer::new(SincResampler::new()) }
fn mk_player<'a>() -> ChannelPlayer<'a, DirectFFTResampler> {  ChannelPlayer::new(DirectFFTResampler::new()) }
//fn mk_player<'a>() -> ChannelPlayer<'a, LinearResampler> {  ChannelPlayer::new(LinearResampler::new()) }

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
	mk_player(),
	mk_player(),
	mk_player(),
	mk_player(),
    ];

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
