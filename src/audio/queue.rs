#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::collections::VecDeque;
use std::rc::Rc;
use std::ops::DerefMut;

use super::ArcIt;
use super::dsp::frequency_range::Freq;
use super::dsp::vtracker::TrackerSensor;
use super::dsp::writer::FlexPCMWriter;
use super::dsp::writer::FlexPCMResult;
use super::dsp::frequency_range::FreqRange;
use super::iterator::AQOp;
use super::iterator::AQSample;
pub use super::samplesource::SampleRange;
use super::samplesource::SampleSource;
use super::samplesource::SampleWriter;

#[cfg(test)]
use super::samplesource::SimpleSampleSource;

#[cfg(test)]
use crate::audio::iterator;

pub trait AudioIteratorProcessor {
    fn flush(&mut self);
    fn set_source(&mut self, new_it : ArcIt);
}

pub struct AudioQueue {
    sample_source : Rc<dyn SampleSource>,
    current_sample : SampleWriter, // sample to play right now
    current_sample_vec : VecDeque<AQSample>,   // enqueued sapmles

    audio_source : ArcIt,
    queue : VecDeque<AQOp>,  // unprocessed AQOps
    flush_requested : bool,
    freq : Freq,
    // next_freq : Freq,
    volume : f32,
    remaining_secs : f64,    // seconds during which the current state applies
    tracker : TrackerSensor,
}

impl AudioQueue {
    #[cfg(test)]
    pub fn nw(audio_source : ArcIt, sample_source : Rc<dyn SampleSource>) -> AudioQueue {
	return AudioQueue::new(audio_source, sample_source, TrackerSensor::new());
    }

    pub fn new(audio_source : ArcIt, sample_source : Rc<dyn SampleSource>, tracker : TrackerSensor) -> AudioQueue {
	return AudioQueue {
	    sample_source,
	    current_sample : SampleWriter::empty(),
	    current_sample_vec : VecDeque::new(),

	    audio_source,
	    queue : VecDeque::new(),
	    flush_requested : false,
	    freq : 1,
	    // next_freq : 1,
	    volume : 1.0,
	    remaining_secs : 0.0,
	    tracker,
	}
    }

    fn soft_reset(&mut self) {
	self.current_sample_vec = VecDeque::new();
	self.queue.truncate(0);
	self.remaining_secs = self.secs_remaining_in_sample();
    }

    fn hard_reset(&mut self) {
	self.soft_reset();
	self.remaining_secs = 0.0;
	self.current_sample = SampleWriter::empty();
    }

    pub fn secs_remaining_in_sample(&self) -> f64 {
	return self.current_sample.remaining() as f64 / self.freq as f64;
    }

    fn poll_iterator_into_queue(&mut self) {
	let mut guard = self.audio_source.lock().unwrap();
	let src = guard.deref_mut();
	src.next(&mut self.queue);
    }

    /// Returns the next frequency to set, if any
    fn update_state_from_next_queue_items(&mut self) -> Option<Freq> {
	let mut retval = None;
        loop {
	    let action = self.queue.pop_front();
	    info!("[AQ]  ::update: {action:?}");
	    match action {
		Some(AQOp::WaitMillis(0))      => { },
		Some(AQOp::WaitMillis(millis)) => { self.remaining_secs += millis as f64 * INV_1000;
						    break; },
		Some(AQOp::SetSamples(svec))   => { self.set_sample_vec(svec); },
		//self.current_sample = SampleWriter::empty()
		// Some(AQOp::SetFreq(freq))      => { self.next_freq = freq; }
		Some(AQOp::SetFreq(freq))      => { self.freq = freq;
						    retval = Some(freq); }
		Some(AQOp::SetVolume(vol))     => { self.volume = vol; },
		None => { break; },
	    }
	}
	return retval;
    }

    fn set_sample_vec(&mut self, svec : Vec<AQSample>) {
	self.current_sample_vec.clear();
	for x in svec {
	    if let AQSample::OnceAtOffset(samplerange, None) = x {
		self.current_sample_vec.push_back(AQSample::OnceAtOffset(samplerange, Some(self.current_sample.get_offset())));
	    } else {
		self.current_sample_vec.push_back(x);
	    }
	}
	self.stop_sample();
    }

    fn stop_sample(&mut self) {
	self.current_sample = SampleWriter::empty();
    }

    fn sample_stopped(&self) -> bool {
	return self.current_sample.len() == 0;
    }
}

const INV_1000 : f64 = 0.001;

impl FlexPCMWriter for AudioQueue {
    fn write_flex_pcm(&mut self, outbuf : &mut [f32], freqrange : &mut FreqRange, msecs_requested : usize) -> FlexPCMResult {
	if self.flush_requested {
	    self.flush_requested = false;
	    info!("[AQ] => Flush");
	    return FlexPCMResult::Flush;
	}

	let mut outbuf_pos = 0;
	let mut secs_written = 0.0;
	let secs_requested = msecs_requested as f64 * INV_1000;
	let outbuf_len = outbuf.len();
	debug!("[AQ] Asked for {secs_requested}s or {outbuf_len} samples");
	while secs_written < secs_requested && outbuf_pos < outbuf_len {
	    // At the current frequency, how many msecs can we fit into the buffer?
	    let max_outbuf_write = outbuf_len - outbuf_pos;
	    let max_outbuf_write_sec = max_outbuf_write as f64 / self.freq as f64;
	    let max_secs_to_write = f64::min(max_outbuf_write_sec, msecs_requested as f64 * INV_1000 - secs_written);
	    let secs_to_write = f64::min(max_secs_to_write, self.remaining_secs);

	    trace!("[AQ] f={} Hz  vol={}  secs_remaining={}  samples_left={}",
		     self.freq, self.volume, self.remaining_secs, self.current_sample.remaining());
	    trace!("[AQ] available in out buffer: time:{max_secs_to_write} space:{max_outbuf_write}");

	    if self.remaining_secs > 0.0 {
		// We should write the current sample information
		if self.current_sample.done() {
		    if !self.sample_stopped() {
			debug!("[AQ] Sample finishes");
			self.stop_sample();
		    }
		    // if self.next_freq != self.freq {
		    // 	trace!("[AQ] Freq change {} -> {} at {outbuf_pos}", self.freq, self.next_freq);
		    // 	freqrange.append(outbuf_pos, self.next_freq);
		    // 	self.freq = self.next_freq;
		    // }

		    let opt_range = match self.current_sample_vec.pop_front() {
			Some(AQSample::Once(range)) => Some(range),
			Some(AQSample::OnceAtOffset(range, Some(off)))
			                            => Some(range.at_offset(off)),
			Some(AQSample::OnceAtOffset(_, None))
			                            => panic!("Unexpected"),
			Some(AQSample::Loop(range)) => { self.current_sample_vec.push_front(AQSample::Loop(range));
							 Some(range) },
			None                        => None,
		    };
		    if let Some(range) = opt_range {
			self.current_sample = self.sample_source.get_sample(range);
		    }

		}
		let mut secs_written_this_round = secs_to_write;
		let num_samples_to_write_by_secs = f64::ceil(secs_to_write * self.freq as f64) as usize;
		if !self.current_sample.done() {
		    // Waiting and have current sample information
		    let samples_remaining = usize::min(max_outbuf_write, self.current_sample.remaining());
		    let num_samples_to_write;
		    if num_samples_to_write_by_secs > samples_remaining {
			num_samples_to_write = samples_remaining;
			secs_written_this_round = num_samples_to_write as f64 / self.freq as f64;
		    } else {
			num_samples_to_write = num_samples_to_write_by_secs;
		    }
		    trace!("[AQ] writing min(time:{num_samples_to_write_by_secs}, src&dest-space:{samples_remaining}) = {num_samples_to_write}");
		    self.current_sample.write(&mut outbuf[outbuf_pos..outbuf_pos+num_samples_to_write]);
		    let vol = self.volume;
		    let mut accumulator = 0.0;
		    if vol != 1.0 {
			for x in outbuf[outbuf_pos..outbuf_pos+num_samples_to_write].iter_mut() {
			    let v = *x * vol;
			    *x = v;
			    accumulator += f32::abs(v);
			}
			self.tracker.add_many(accumulator, num_samples_to_write);
		    }
		    outbuf_pos += num_samples_to_write;
		} else {
		    trace!("[AQ] ** out of time");
		    // Waiting but no current sample information?  Write silence.
		    let num_zeroes_to_write;
		    if num_samples_to_write_by_secs > max_outbuf_write {
			num_zeroes_to_write = max_outbuf_write;
			secs_written_this_round = num_zeroes_to_write as f64 / self.freq as f64;
		    } else {
			num_zeroes_to_write = num_samples_to_write_by_secs;
		    }

		    outbuf[outbuf_pos..outbuf_pos+num_zeroes_to_write].fill(0.0);
		    outbuf_pos += num_zeroes_to_write;
		}
		secs_written += secs_written_this_round;
		self.remaining_secs -= secs_written_this_round;
		trace!{"[AQ] written: secs {secs_written}/{secs_requested}; bytes {outbuf_pos}/{outbuf_len}"}
	    } else {
		// Waiting for the audio iterator to send WaitMillis
		if self.queue.len() == 0 {
		    self.poll_iterator_into_queue();
		}
		if self.queue.len() == 0 {
		    // Iterator has given up on us?
		    if outbuf_pos == 0 {
			trace!("[AQ] => Silence");
			return FlexPCMResult::Silence;
		    } else {
			trace!("[AQ] ** early abort: => Wrote({outbuf_pos})!");
			return FlexPCMResult::Wrote(outbuf_pos);
		    }
		}
		match self.update_state_from_next_queue_items() {
		    Some(new_freq) => freqrange.append(outbuf_pos, new_freq),
		    None           => {},
		}
	    }
	};
	trace!("[AQ] => Wrote({outbuf_pos})");
	return FlexPCMResult::Wrote(outbuf_pos);
    }

}

impl AudioIteratorProcessor for AudioQueue {
    // Finishes playing the current sample
    fn flush(&mut self) {
	self.hard_reset();
	self.flush_requested = true;
    }

    fn set_source(&mut self, source : ArcIt) {
	// commented out: stopping the current sample will probably not yield good results
	//self.current_sample = SampleWriter::empty();
	self.audio_source = source;
	self.soft_reset();
	info!("[AQ] ** New iterator installed -> {} s remain ({} / {}))",
	      self.remaining_secs,
	      self.current_sample.remaining(), self.freq);
	self.flush();
    }
}

// ----------------------------------------

#[cfg(test)]
fn setup_samplesource() -> Rc<dyn SampleSource> {
    let mut v = vec![];
    for i in 1..1000 {
	v.push(i as f32);
    }
    return Rc::new(SimpleSampleSource::from_vec_f32(v));
}

#[cfg(test)]
#[test]
fn test_default_silence_bufsize_limited() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(100),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 100);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    assert_eq!((100, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_default_silence_time_limited() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 4);
    assert_eq!(FlexPCMResult::Wrote(4), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_bufsize_limited() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 10);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_time_limited() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 4);
    assert_eq!(FlexPCMResult::Wrote(4), r);
    assert_eq!([1.0, 2.0, 3.0, 4.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_switch() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(2)],
				  vec![
				      AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,20))]),
				      AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 8);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([1.0, 2.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_loop() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(0,2)),
				       ]),
				       AQOp::WaitMillis(20)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 8);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([1.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 2.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_once_loop() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,2)),
							     AQSample::Loop(SampleRange::new(0,3)),
				       ]),
				       AQOp::WaitMillis(20)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 8);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([11.0, 12.0, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_twice_loop() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,2)),
							     AQSample::Once(SampleRange::new(20,1)),
							     AQSample::Loop(SampleRange::new(0,2)),
				       ]),
				       AQOp::WaitMillis(20)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 8);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([11.0, 12.0, 21.0, 1.0, 2.0, 1.0, 2.0, 1.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_freq_switch_sample_boundary() {
    let mut outbuf = [-1.0; 10];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,2))]),
				       AQOp::WaitMillis(2),
				       AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,4))]),
				       AQOp::WaitMillis(1),
				       AQOp::SetFreq(500),
				       AQOp::WaitMillis(1),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(20,10))]),
				       AQOp::WaitMillis(20),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 7);
    // expect hard switches
    assert_eq!([1.0, 2.0, 11.0, 12.0, 13.0, 21.0, 22.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);
    assert_eq!(FlexPCMResult::Wrote(7), r);
    assert_eq!((1000, Some(2)),  freqrange.get(0));
    assert_eq!((2000, Some(2)),  freqrange.get(2));
    assert_eq!((500, None),      freqrange.get(6));
}

#[cfg(test)]
#[test]
fn test_volume() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,20))]),
				       AQOp::SetVolume(10.0),
				       AQOp::WaitMillis(2),
				       AQOp::SetVolume(1.0),
				       AQOp::WaitMillis(2),
				       AQOp::SetVolume(2.0),
				       AQOp::WaitMillis(20),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 8);
    assert_eq!(FlexPCMResult::Wrote(8), r);
    assert_eq!([10.0, 20.0, 3.0, 4.0, 10.0, 12.0, 14.0, 16.0],
	       &outbuf[..]);
    assert_eq!((1000, None),  freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_run_out() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(5),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 10);
    assert_eq!(FlexPCMResult::Wrote(5), r);
    let r = aq.write_flex_pcm(&mut outbuf[5..], &mut freqrange, 10);
    assert_eq!(FlexPCMResult::Silence, r);
    assert_eq!([1.0, 2.0, 3.0, 0.0, 0.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);
    assert_eq!((1000, None),  freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_replace_iterator() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,4))]),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();

    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 1);
    assert_eq!(FlexPCMResult::Wrote(2), r);
    assert_eq!((2000, None),
	       freqrange.get(0));
    assert_eq!([11.0, 12.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);

    let ait2 = iterator::mock(vec![vec![AQOp::SetFreq(1000),
					AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
					AQOp::WaitMillis(1000)]]);
    aq.set_source(ait2);
    let r = aq.write_flex_pcm(&mut outbuf[2..], &mut freqrange.at_offset(2), 10);
    assert_eq!(FlexPCMResult::Flush, r);
    let r = aq.write_flex_pcm(&mut outbuf[2..], &mut freqrange.at_offset(2), 10);

    assert_eq!([11.0, 12.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
	       &outbuf[..]);
    assert_eq!((2000, Some(2)),
	       freqrange.get(0));
    assert_eq!((1000, None),
	       freqrange.get(2));
    assert_eq!(FlexPCMResult::Wrote(6), r);
}

#[cfg(test)]
#[test]
fn test_sample_loop_interrupted() {

    let mut outbuf = [-1.0; 12];

    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(2),
				       AQOp::SetVolume(100.0),
				       AQOp::WaitMillis(2),
				       AQOp::SetFreq(1999), // 2000 seems to introduce fp imprecision?
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(10,3))]),
				       AQOp::WaitMillis(4),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange, 7);
    assert_eq!([1.0, 2.0, 300.0, 100.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0, 1300.0, -1.0, -1.0],
	       &outbuf[..]);
    assert_eq!((1000, Some(4)),
	       freqrange.get(0));
    assert_eq!((1999, None),
	       freqrange.get(4));
    assert_eq!(FlexPCMResult::Wrote(10), r);
}
