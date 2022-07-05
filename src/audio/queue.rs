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
    current_sample_vec : VecDeque<AQSample>,
    current_sample : Option<SampleWriter<'a>>,

    audio_source : &'a mut dyn AudioIterator,
    queue : VecDeque<AQOp>,
    updated_source : bool,
    freq : Freq,
    volume : f32,
    msecs_left : f64,
}

impl<'a> AudioQueue<'a> {
    pub fn new(audio_source : &'a mut dyn AudioIterator, sample_source : &'a dyn SampleSource) -> AudioQueue<'a> {
	return AudioQueue {
	    sample_source,
	    current_sample_vec : VecDeque::new(),
	    updated_source : true,

	    audio_source,
	    queue : VecDeque::new(),
	    freq : 0,
	    volume : 1.0,
	    current_sample : None,
	    msecs_left : 0.0,
	}
    }

    pub fn set_source(&mut self, source : &'a mut dyn AudioIterator) {
	self.updated_source = true;
	self.queue.truncate(0);
	self.current_sample = None;
	self.audio_source = source;
    }
}

impl<'a> FlexPCMWriter for AudioQueue<'a> {
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange, msecs_requested : usize) -> FlexPCMResult {
	if self.updated_source {
	    self.updated_source = false;
	    return FlexPCMResult::Flush;
	}

	let mut write_pos = 0;
	let mut msecs_written = 0.0;
	while msecs_written < msecs_requested as f32 && write_pos < output.len() {
	    if let mut sample = self.current_sample {
	    } else {
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
			Some(AQOp::WaitMillis(millis)) => { self.msecs_left += millis as f64;
							    break; },
			Some(AQOp::SetSamples(svec))   => { self.current_sample_vec = VecDeque::from(svec);
							    break; },
			Some(AQOp::SetFreq(freq))      => { self.freq = freq;
							    freqrange.append(write_pos, freq); },
			Some(AQOp::SetVolume(vol))     => { self.volume = vol; },
			None => { break; }, // Go back and iterate as needed
		    }
		}
	    }
	};
	return FlexPCMResult::Wrote(write_pos);
    }
}
