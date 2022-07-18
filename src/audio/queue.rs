// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::ops::DerefMut;

use super::ArcIt;
use super::dsp::frequency_range::Freq;
use super::dsp::vtracker::TrackerSensor;
use super::dsp::writer::PCMFlexWriter;
use super::dsp::writer::SyncPCMResult;
use super::dsp::frequency_range::FreqRange;
use super::dsp::writer::Timeslice;
use super::iterator::AQOp;
use super::iterator::AQSample;
pub use super::samplesource::SampleRange;
use super::samplesource::SampleSource;
use super::samplesource::SampleWriter;

#[cfg(test)]
use super::samplesource::SimpleSampleSource;

// #[cfg(test)]
// use crate::audio::iterator;

pub trait AudioIteratorProcessor {
    fn flush(&mut self);
    fn set_source(&mut self, new_it : ArcIt);
}

pub struct AudioQueue {
    sample_source : Rc<RefCell<dyn SampleSource>>,
    current_sample : SampleWriter, // sample to play right now
    current_sample_vec : VecDeque<AQSample>,   // enqueued sapmles

    audio_source : ArcIt,
    queue : VecDeque<AQOp>,  // unprocessed AQOps
    flush_requested : bool,
    play_freq : Freq, // Current play frequency for samples

    timeslice : Option<Timeslice>, // If Some(_), prevents further queue polling until we've been advanced
    have_reported_timeslice_update : bool, // true after we report timeslice and before the client advances to the next timeslice
    volume : f32,
    remaining_secs : f64,    // seconds during which the current state applies
    tracker : TrackerSensor,
}

impl AudioQueue {
    #[cfg(test)]
    fn nw(audio_source : ArcIt, sample_source : Rc<RefCell<dyn SampleSource>>) -> AudioQueue {
	return AudioQueue::new(audio_source, sample_source, TrackerSensor::new());
    }

    pub fn new(audio_source : ArcIt, sample_source : Rc<RefCell<dyn SampleSource>>, tracker : TrackerSensor) -> AudioQueue {
	return AudioQueue {
	    sample_source,
	    current_sample : SampleWriter::empty(),
	    current_sample_vec : VecDeque::new(),

	    audio_source,
	    queue : VecDeque::new(),
	    flush_requested : false,
	    play_freq : 1,

	    timeslice : None,
	    have_reported_timeslice_update : false,
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
	return self.current_sample.remaining_secs();
    }

    fn poll_iterator_into_queue(&mut self) {
	let mut guard = self.audio_source.lock().unwrap();
	let src = guard.deref_mut();
	src.next(&mut self.queue);
    }

    /// Returns the next frequency to set, if any
    fn update_state_from_next_queue_items(&mut self) -> Option<Freq> {
	let mut retval = None;
	// Timeslices must be enabled through a separate API call to advance_sync(), so don't query
	// if we are waiting for that
	while !self.waiting_for_next_timeslice() {
	    let action = self.queue.pop_front();
	    pinfo!("[AQ]  ::update: {action:?}");
	    match action {
		Some(AQOp::WaitMillis(0))      => { },
		Some(AQOp::WaitMillis(millis)) => { self.remaining_secs += millis as f64 * INV_1000;
						    break; },
		Some(AQOp::Timeslice(tslice))  => { self.timeslice = Some(tslice);
						    break; },
		Some(AQOp::SetSamples(svec))   => { self.set_sample_vec(svec); },
		Some(AQOp::SetFreq(freq))      => { self.play_freq = freq;
						    retval = Some(freq); }
		Some(AQOp::SetVolume(vol))     => { self.volume = vol; },
		Some(AQOp::End)                => { },
		None => { break; },
	    }
	}
	return retval;
    }

    fn set_sample_vec(&mut self, svec : Vec<AQSample>) {
	self.current_sample_vec.clear();
	for x in svec {
	    if let AQSample::OnceAtOffset(samplerange, None) = x {
		self.current_sample_vec.push_back(AQSample::OnceAtOffset(samplerange, Some((self.current_sample.get_offset(), 0))));
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

    fn waiting_for_next_timeslice(&self) -> bool {
	return self.timeslice != None;
    }

    fn success(&mut self, written : usize) -> SyncPCMResult {
	if let Some(_) = self.timeslice {
	    self.have_reported_timeslice_update = true;
	}
	ptrace!("-------> Wrote(({written}, {:?}))", self.timeslice);
	return SyncPCMResult::Wrote(written, self.timeslice);
    }

    fn newly_at_timeslice_boundary(&self) -> bool {
	return self.waiting_for_next_timeslice() && !self.have_reported_timeslice_update;
    }

    fn have_bounded_time(&self) -> bool {
	return !self.have_reported_timeslice_update;
    }
}

const INV_1000 : f64 = 0.001;

impl PCMFlexWriter for AudioQueue {
    fn write_flex_pcm(&mut self, outbuf : &mut [f32], freqrange : &mut FreqRange) -> SyncPCMResult {
	if self.flush_requested {
	    self.flush_requested = false;
	    pinfo!("[AQ] => Flush");
	    return SyncPCMResult::Flush;
	}

	let mut outbuf_pos = 0;
	let outbuf_len = outbuf.len();
	pdebug!("[AQ] Asked for {outbuf_len} samples");


	let mut progress = true;
	while outbuf_pos < outbuf_len {
	    // How many samples can we fit into the buffer?
	    let max_outbuf_write = outbuf_len - outbuf_pos;
	    // How much sample timing info do we have remaining?
	    let secs_to_write = // If we have reported the timeslice and yet still get called, the client
		                // gives us leave to write as much as we can
		if self.have_bounded_time() { self.remaining_secs } else { f64::INFINITY };

	    ptrace!("[AQ] play_f={}/sample_f={} Hz  vol={}  secs_remaining={}  samples_left={}",
		    self.play_freq, self.current_sample.get_freq(), self.volume, self.remaining_secs, self.current_sample.remaining());
	    ptrace!("[AQ] available in out buffer: time:{secs_to_write} space:{max_outbuf_write}");
	    if !progress {
		// check_count -= 1;
		// if check_count == 0 {
		panic!("Stuck!");
		// }
	    }
	    progress = false;

	    if secs_to_write > 0.0 {
		// We should write the current sample information
		if self.current_sample.done() {
		    if !self.sample_stopped() {
			pdebug!("[AQ] Sample finishes");
			self.stop_sample();
			progress = true;
		    }

		    let opt_range = match self.current_sample_vec.pop_front() {
			Some(AQSample::Once(range)) => Some(range),
			Some(AQSample::OnceAtOffset(range, Some((off, _))))
			                            => Some(range.at_offset(off)),
			Some(AQSample::OnceAtOffset(_, None))
			                            => panic!("Unexpected"),
			Some(AQSample::Loop(range)) => { self.current_sample_vec.push_front(AQSample::Loop(range));
							 Some(range) },
			None                        => None,
		    };
		    if let Some(range) = opt_range {
			progress = true;
			self.current_sample = self.sample_source.borrow_mut().get_sample(range, self.play_freq);
			freqrange.append(outbuf_pos, self.current_sample.get_freq());
		    }

		}
		let mut secs_written_this_round = secs_to_write;
		let num_samples_to_write_by_secs =
		    if !self.waiting_for_next_timeslice() { f64::ceil(secs_to_write * self.current_sample.get_freq() as f64) as usize } else { max_outbuf_write };
		if self.current_sample.done() {
		    // Waiting but no sample information
		    let samples_to_fill = usize::min(num_samples_to_write_by_secs, max_outbuf_write);
		    if samples_to_fill > 0 {
			outbuf[outbuf_pos..outbuf_pos+samples_to_fill].fill(0.0); // Fill with the sound of silence
			self.tracker.add_many(0.0, samples_to_fill);
			outbuf_pos += samples_to_fill;
			progress = true;
		    }
		} else {
		    // Waiting and have current sample information
		    let samples_remaining = usize::min(max_outbuf_write, self.current_sample.remaining());
		    let num_samples_to_write;
		    if num_samples_to_write_by_secs > samples_remaining {
			num_samples_to_write = samples_remaining;
			secs_written_this_round = num_samples_to_write as f64 / self.current_sample.get_freq() as f64;
		    } else {
			num_samples_to_write = num_samples_to_write_by_secs;
		    }
		    ptrace!("[AQ] writing min(time:{num_samples_to_write_by_secs}, src&dest-space:{samples_remaining}) = {num_samples_to_write}");
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
		    if num_samples_to_write > 0 {
			progress = true;
		    }
		}
		// if self.have_bounded_time() {
		//     ptrace!("Filling up the rest with zeroes");
		//     outbuf[outbuf_pos..].fill(0.0); // Fill with the sound of silence
		// }
		self.remaining_secs -= secs_written_this_round;
		ptrace!{"[AQ] written: {outbuf_pos}/{outbuf_len}; remaining secs - {secs_written_this_round} = {}", self.remaining_secs}
	    } else {
		// Waiting for the audio iterator to send WaitMillis
		if self.queue.len() == 0 {
		    self.poll_iterator_into_queue();
		}
		if self.queue.len() > 0 {
		    progress = true;
		} else {
		    // Iterator has given up on us?
		    ptrace!("[AQ] => Silence");
		    if self.newly_at_timeslice_boundary() {
			self.success(outbuf_pos);
		    }
		    outbuf[outbuf_pos..].fill(0.0); // Fill with the sound of silence
		    return self.success(outbuf_len);
		}
		match self.update_state_from_next_queue_items() {
		    Some(new_freq) => { self.play_freq = new_freq;
		                        progress = true; },
		    None           => {},
		}
		if self.waiting_for_next_timeslice() && !self.have_reported_timeslice_update {
		    return self.success(outbuf_pos);
		}
	    }
	};
	ptrace!("[AQ] => Wrote({outbuf_pos})");
	return self.success(outbuf_pos);
    }

    fn advance_sync(&mut self, timeslice : super::dsp::writer::Timeslice) {
	assert_eq!(self.timeslice, Some(timeslice));
	self.have_reported_timeslice_update = false;
	self.remaining_secs = 0.0;
	self.timeslice = None;
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
	pinfo!("[AQ] ** New iterator installed -> {} s remain ({} / {}))",
	      self.remaining_secs,
	      self.current_sample.remaining(), self.current_sample.remaining_secs());
	self.flush();
    }
}

// ========================================
// Testing

#[cfg(test)]
use crate::audio::iterator;

// ----------------------------------------
// Test helpers

#[cfg(test)]
fn setup_samplesource() -> Rc<RefCell<dyn SampleSource>> {
    let mut v = vec![];
    for i in 1..1000 {
	v.push(i as f32);
    }
    return Rc::new(RefCell::new(SimpleSampleSource::from_vec_f32(v)));
}

// ----------------------------------------
// Tests

#[ignore]
#[cfg(test)]
#[test]
fn test_default_silence() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(100),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    assert_eq!((100, None),
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_sample_switch() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,5))]),
				       AQOp::WaitMillis(2)],
				  vec![
				      AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,20))]),
				      AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([11.0, 12.0, 21.0, 1.0, 2.0, 1.0, 2.0, 1.0],
	       &outbuf[..]);
    assert_eq!((1000, None),
	       freqrange.get(0));
}

#[ignore]
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    // expect hard switches
    assert_eq!([1.0, 2.0, 11.0, 12.0, 13.0, 21.0, 22.0, 23.0, 24.0, 25.0],
	       &outbuf[..]);
    assert_eq!(SyncPCMResult::Wrote(10, None), r);
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
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([10.0, 20.0, 3.0, 4.0, 10.0, 12.0, 14.0, 16.0],
	       &outbuf[..]);
    assert_eq!((1000, None),  freqrange.get(0));
}

#[cfg(test)]
#[test]
fn test_run_out() {
    let mut outbuf0 = [-1.0; 8];
    let mut outbuf1 = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(5),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf0, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf0[..]);
    let r = aq.write_flex_pcm(&mut outbuf1, &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf1[..]);
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

    let r = aq.write_flex_pcm(&mut outbuf[0..2], &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(2, None), r);
    assert_eq!((2000, None),
	       freqrange.get(0));
    assert_eq!([11.0, 12.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);

    let ait2 = iterator::mock(vec![vec![AQOp::SetFreq(1000),
					AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
					AQOp::WaitMillis(1000)]]);
    aq.set_source(ait2);
    let r = aq.write_flex_pcm(&mut outbuf[2..], &mut freqrange.at_offset(2));
    assert_eq!(SyncPCMResult::Flush, r);
    let r = aq.write_flex_pcm(&mut outbuf[2..], &mut freqrange.at_offset(2));

    assert_eq!([11.0, 12.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
	       &outbuf[..]);
    assert_eq!((2000, Some(2)),
	       freqrange.get(0));
    assert_eq!((1000, None),
	       freqrange.get(2));
    assert_eq!(SyncPCMResult::Wrote(6, None), r);
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
				       AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(10,3))]),
				       AQOp::WaitMillis(4),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();
    let r = aq.write_flex_pcm(&mut outbuf, &mut freqrange);
    assert_eq!([1.0, 2.0, 300.0, 100.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0],
	       &outbuf[..]);
    assert_eq!((1000, Some(4)),
	       freqrange.get(0));
    assert_eq!((2000, None),
	       freqrange.get(4));
    assert_eq!(SyncPCMResult::Wrote(12, None), r);
}

#[cfg(test)]
#[test]
fn test_wait_on_timeslice() {
    let mut outbuf = [-1.0; 20];

    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(2),
				       AQOp::SetVolume(100.0),
				       AQOp::Timeslice(1),

				       AQOp::SetVolume(1000.0),
				       AQOp::WaitMillis(1),
				       AQOp::SetVolume(1.0),
				       AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(10,3))]),
				       AQOp::WaitMillis(1),
				       AQOp::Timeslice(2),
				       AQOp::SetVolume(0.5),

				       AQOp::WaitMillis(3),
				       AQOp::Timeslice(3),
    ]]);
    let ssrc = setup_samplesource();
    let mut aq = AudioQueue::nw(ait, ssrc);
    let mut freqrange = FreqRange::new();

    let r = aq.write_flex_pcm(&mut outbuf[0..1], &mut freqrange);
    assert_eq!(SyncPCMResult::Wrote(1, None), r);
    let r = aq.write_flex_pcm(&mut outbuf[1..5], &mut freqrange.at_offset(1));
    assert_eq!(SyncPCMResult::Wrote(1, Some(1)), r);
    let r = aq.write_flex_pcm(&mut outbuf[2..5], &mut freqrange.at_offset(2));
    assert_eq!(SyncPCMResult::Wrote(3, Some(1)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		-1.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..10]);

    aq.advance_sync(1);

    let r = aq.write_flex_pcm(&mut outbuf[5..], &mut freqrange.at_offset(5));
    assert_eq!(SyncPCMResult::Wrote(3, Some(2)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		3000.0, 11.0, 12.0,
		// ts-2 available
		-1.0, -1.0],
	       &outbuf[..10]);

    let r = aq.write_flex_pcm(&mut outbuf[8..10], &mut freqrange.at_offset(8));
    assert_eq!(SyncPCMResult::Wrote(2, Some(2)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		3000.0, 11.0, 12.0,
		// ts-2 available
		13.0, 11.0,
		-1.0],
	       &outbuf[..11]);

    aq.advance_sync(2);

    let r = aq.write_flex_pcm(&mut outbuf[10..], &mut freqrange.at_offset(10));
    assert_eq!(SyncPCMResult::Wrote(6, Some(3)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		3000.0, 11.0, 12.0,
		// ts-2 available
		13.0, 11.0,
		// ts-2 active
		6.0, 6.5, 5.5, 6.0, 6.5, 5.5,
		// ts-3 available
		-1.0,
		-1.0,
		-1.0,
		-1.0,    ],
	       &outbuf[..]);

    assert_eq!((1000, Some(6)),
	       freqrange.get(0));
    assert_eq!((2000, None),
	       freqrange.get(6));
}
