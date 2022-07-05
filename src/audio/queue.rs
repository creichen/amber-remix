use std::collections::VecDeque;

use super::dsp::frequency_range::Freq;
use super::dsp::writer::FlexPCMWriter;
use super::dsp::writer::FlexPCMResult;
use super::dsp::frequency_range::FreqRange;
pub use super::samplesource::SampleRange;
use super::samplesource::SampleSource;
use super::samplesource::SampleWriter;

/**
 * Audio queue operations allow AudioIterators to control output to their channel.
 *
 * "X ; WaitMillis(n); Y" means that settings X will be in effect for "n" milliseconds,
 * then any changes from Y take effect.
 */
#[derive(Clone)]
pub enum AQOp {
    /// Process channel settings for specified nr of milliseconds
    WaitMillis(usize),
    /// Enqueue to the sample queue
    SetSamples(Vec<AQSample>),
    /// Set audio frequency in Hz
    SetFreq(Freq),
    /// Set audio volume as fraction
    SetVolume(f32),
}

#[derive(Clone, Copy)]
pub enum AQSample {
    /// Loop specified sample
    Loop(SampleRange),
    /// Play specified sample once
    Once(SampleRange),
}

pub trait AudioIterator : Send + Sync {
    fn next(&mut self, queue : &mut VecDeque<AQOp>);
}

pub struct AudioQueue<'a> {
    sample_source : &'a dyn SampleSource,
    current_sample_vec : VecDeque<AQSample>,   // enqueued sapmles
    current_sample : Option<SampleWriter<'a>>, // sample to play right now

    audio_source : &'a mut dyn AudioIterator,
    queue : VecDeque<AQOp>,  // unprocessed AQOps
    audio_source_was_updated : bool,
    freq : Freq,
    volume : f32,
    remaining_secs : f64,    // seconds during which the current state applies
}

impl<'a> AudioQueue<'a> {
    pub fn new(audio_source : &'a mut dyn AudioIterator, sample_source : &'a dyn SampleSource) -> AudioQueue<'a> {
	return AudioQueue {
	    sample_source,
	    current_sample_vec : VecDeque::new(),
	    audio_source_was_updated : true,

	    audio_source,
	    queue : VecDeque::new(),
	    freq : 1,
	    volume : 1.0,
	    current_sample : None,
	    remaining_secs : 0.0,
	}
    }

    pub fn set_source(&mut self, source : &'a mut dyn AudioIterator) {
	self.audio_source_was_updated = true;
	self.queue.truncate(0);
	self.current_sample = None;
	self.audio_source = source;
    }
}

impl<'a> FlexPCMWriter for AudioQueue<'a> {
    fn write_flex_pcm(&mut self, outbuf : &mut [f32], freqrange : &mut FreqRange, msecs_requested : usize) -> FlexPCMResult {
	if self.audio_source_was_updated {
	    self.audio_source_was_updated = false;
	    return FlexPCMResult::Flush;
	}

	let mut outbuf_pos = 0;
	let mut msecs_written = 0.0;
	let outbuf_len = outbuf.len();
	while msecs_written < msecs_requested as f32 && outbuf_pos < outbuf_len {
	    // At the current frequency, how many msecs can we fit into the buffer?
	    let max_outbuf_write_sec = (outbuf_len - outbuf_pos) as f64 / self.freq as f64;
	    let max_secs_to_write = f64::min(max_outbuf_write_sec, msecs_requested as f64 / 1000.0);
	    let secs_to_write = f64::min(max_secs_to_write, self.remaining_secs);

	    if self.remaining_secs > 0.0 {
		// We should write the current sample information
		if !self.current_sample.is_some() {
		    let opt_range = match self.current_sample_vec.pop_front() {
			Some(AQSample::Once(range)) => Some(range),
			Some(AQSample::Loop(range)) => { self.current_sample_vec.push_front(AQSample::Loop(range));
							 Some(range) },
			None                        => None,
		    };
		    if let Some(range) = opt_range {
			self.current_sample = Some(self.sample_source.get_sample(range));
		    }
		}
		if let Some(mut sample) = self.current_sample {
		    // Waiting and have current sample information
		    let num_samples_to_write = usize::min(f64::ceil(secs_to_write * self.freq as f64) as usize,
							  sample.remaining());
		    sample.write(&mut outbuf[outbuf_pos..outbuf_pos+num_samples_to_write]);
		    if sample.done() {
			self.current_sample = None;
		    }
		} else {
		    // Waiting but no current sample information?  Write silence.
		    let num_zeroes_to_write = usize::min(f64::ceil(secs_to_write * self.freq as f64) as usize,
							 outbuf_len - outbuf_pos);
		    &outbuf[outbuf_pos..outbuf_pos+num_zeroes_to_write].fill(0.0);
		    outbuf_pos += num_zeroes_to_write;
		}
	    } else {
		// Waiting for the audio iterator to send WaitMillis
		if self.queue.len() == 0 {
		    self.audio_source.next(&mut self.queue);
		}
		if self.queue.len() == 0 {
		    // Iterator has given up on us?
		    return FlexPCMResult::Silence;
		}
		loop {
		    match self.queue.pop_front() {
			Some(AQOp::WaitMillis(0))      => { },
			Some(AQOp::WaitMillis(millis)) => { self.remaining_secs += millis as f64 * 1000.0;
							    break; },
			Some(AQOp::SetSamples(svec))   => { self.current_sample_vec = VecDeque::from(svec); },
			Some(AQOp::SetFreq(freq))      => { self.freq = freq;
							    freqrange.append(outbuf_pos, freq); },
			Some(AQOp::SetVolume(vol))     => { self.volume = vol; },
			None => { break; }, // Go back and iterate as needed
		    }
		}
	    }
	};
	return FlexPCMResult::Wrote(outbuf_pos);
    }
}

// ----------------------------------------
