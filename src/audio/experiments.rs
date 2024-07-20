
#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
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
pub const NUM_OUTPUT_CHANNELS : usize = 1; // 2 for stereo


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
	    return InstrumentUpdate::Loop;
	}
	// otherwise we have an actual update
	let sample = self.current_sample();
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
	for i in 0..NUM_OUTPUT_CHANNELS {
	    buf[pos * NUM_OUTPUT_CHANNELS + i] = sine;
	}
    }
}

// ================================================================================

trait ChannelPlayer<'a> {
    fn play(&mut self, dest: &mut [f32]);
    fn set_frequency(&mut self, freq: usize);
    fn set_instrument(&mut self, instr: Instrument<'a>);
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
}

// ================================================================================
// SincResamplingPlayer

struct SincResamplingPlayer<'a> {
    freq: usize,
    current_sample: Vec<f32>,
    current_resampled_sample: Vec<f32>,
    current_outpos: usize,
    instrument: Option<Instrument<'a>>,
}

impl<'a> SincResamplingPlayer<'a> {
    fn new() -> Self {
	SincResamplingPlayer {
	    freq: 0,
	    current_sample: vec![0.0],
	    current_resampled_sample: vec![0.0],
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
	println!("Converted {} samples at freq {} to length {}, ratio={resample_ratio}",
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
	if self.freq == 0 {
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
		buf[x + pos] = self.current_resampled_sample[x + self.current_outpos];
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
}


/// Iterate over the song's poly iterator until the buffer is full
pub fn song_to_pcm(sample_data: &SampleData,
		   buf: &mut [f32],
		   song: &Song,
		   sample_rate: usize) {
    let mut poly_it = SongIterator::new(&song,
					song.songinfo.first_division,
					song.songinfo.last_division);

    let max_pos = buf.len();
    let duration_milliseconds = (max_pos * 1000) / sample_rate;
    let mut buf_pos_ms = 0;
    let channel_to_play = 0;
    let mut player = SincResamplingPlayer::new();

    // FIXME: doesn't necessarily iterate until buffer is full
    while buf_pos_ms < duration_milliseconds {
	let mut d = VecDeque::<AQOp>::new();
	let mut d2 = VecDeque::<AQOp>::new();

	for i in 0..4 {
	    if i == channel_to_play {
		poly_it.channels[i].next(&mut d);
		if poly_it.channels[i].is_done() {
		    // FIXME: this should happen when ALL channels are done
		    // (though that should normally coincide.....)
		    poly_it.next_division();
		}
	    } else {
		poly_it.channels[i].next(&mut d2);
	    }
	}

	println!("--- tick {buf_pos_ms:02x}\n");
	for dd in d {
	    println!("  {dd:?}\n");
	    match dd {
		AQOp::SetSamples(samples) => {
		    player.set_instrument(Instrument::new(sample_data,
							  samples));
		},
		AQOp::WaitMillis(ms) => {
		    let start = (SAMPLE_RATE * buf_pos_ms) / 1000;
		    buf_pos_ms += ms;
		    let mut stop = (SAMPLE_RATE * buf_pos_ms) / 1000;
		    if stop > max_pos {
			stop = max_pos;
		    }
		    player.play(&mut buf[start..stop]);
		    // mk_audio(&sample_data,
		    // 	     buf,
		    // 	     &mut current_instrument,
		    // 	     &mut last_instr_sample,
		    // 	     start,
		    // 	     stop - start,
		    // 	     freq);
		},
		AQOp::SetFreq(f) => // freq = f / 32,
		    player.set_frequency(f), // FIXME: workaround for period_to_freq
		//AQOp::Timeslice => poly_it.adv,
		_ => {},
	    }
	    //println!(" {dd:?}\n");
	}
    }
}
