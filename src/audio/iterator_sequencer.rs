// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Sequences a song iterator into a sequence of PCM data

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::{collections::VecDeque, ops::DerefMut, rc::Rc, cell::RefCell};
use crate::audio::dsp::writer::SyncPCMResult;
use super::{dsp::writer::{PCMSyncWriter, FrequencyTrait}, queue::AudioIteratorProcessor};
use super::samplesource::SampleSource;
use crate::audio::SampleRange;

// #[cfg(test)]
// use std::cell::RefCell;
// #[cfg(test)]
// use std::rc::Rc;
use super::{samplesource::{SincSampleSource, SampleWriter}, dsp::{writer::Timeslice, vtracker::TrackerSensor}, AQSample, ArcIt, AQOp, Freq};

const DEBUG : bool = true;

// pub trait AudioIteratorProcessor {
//     fn flush(&mut self);
//     fn set_source(&mut self, new_it : ArcIt);
// }

struct TimesliceUpdate {
    timeslice : Timeslice,
    update_propagated : bool, // 'true' once we've reported the update
}

impl TimesliceUpdate {
    fn new(timeslice : Timeslice) -> Option<TimesliceUpdate> {
	return Some(TimesliceUpdate {
	    timeslice,
	    update_propagated : false,
	});
    }

    fn mark_propagated(&mut self) {
	self.update_propagated = true;
    }
}

/// Sequencer state that indicates progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyState {
    Flush,                      // Flush requested
    ContinuingSilence,          // No iterator, nothing to play
    TemporarySilence(usize),    // Nothing to play for (up to) that many samples
    PlaySample(usize),          // Play (up to) this many samples
    ReportTimeslice(Timeslice),
}

pub struct IteratorSequencer {
    sample_source : Rc<RefCell<SincSampleSource>>,
    current_sample : SampleWriter, // sample to play right now
    current_sample_range : Option<SampleRange>,
    current_sample_vec : VecDeque<AQSample>,   // enqueued sapmles

    audio_source : Option<ArcIt>,
    /// FIXME: could this just be usize, given that we try to pull anyway?
    samples_until_next_poll : usize,
    queue : VecDeque<AQOp>,  // unprocessed AQOps
    flush_requested : bool,

    timeslice : Option<TimesliceUpdate>, // If Some(_), prevents further queue polling until we've been advanced
    play_freq : Freq,
    play_volume : f32,

    target_freq : Freq,

    tracker : TrackerSensor,
}

impl IteratorSequencer {
    #[cfg(test)]
    fn nw(audio_source : ArcIt, sample_source : Rc<RefCell<SincSampleSource>>) -> IteratorSequencer {
	return IteratorSequencer::new_with_source(audio_source, 1000, sample_source, TrackerSensor::new());
    }

    pub fn new<'a>(out_freq : Freq, sample_source : Rc<RefCell<SincSampleSource>>, tracker : TrackerSensor) -> IteratorSequencer {
	return IteratorSequencer {
	    sample_source,
	    current_sample : SampleWriter::empty(),
	    current_sample_range : None,
	    current_sample_vec : VecDeque::new(),

	    audio_source : None,
	    samples_until_next_poll : 0,
	    queue : VecDeque::new(),
	    flush_requested : false,

	    timeslice : None,
	    play_volume : 1.0,
	    play_freq : out_freq,

	    target_freq : out_freq,

	    tracker,
	}
    }

    pub fn new_with_source<'a>(songit : ArcIt, out_freq : Freq, sample_source : Rc<RefCell<SincSampleSource>>, tracker : TrackerSensor) -> IteratorSequencer {
	let mut iseq = IteratorSequencer::new(out_freq, sample_source, tracker);
	iseq.set_source(songit);
	return iseq;
    }

    fn soft_reset(&mut self) {
	self.current_sample_vec = VecDeque::new();
	self.queue.truncate(0);
    }

    fn hard_reset(&mut self) {
	self.soft_reset();
	self.current_sample = SampleWriter::empty();
	self.samples_until_next_poll = 0;
    }

    pub fn samples_remaining(&self) -> usize {
	//return self.current_sample.remaining() * self.current_sample.get_freq() / self.target_freq;
	return self.current_sample.remaining() * self.target_freq / self.current_sample.get_freq();
    }

    fn poll_iterator_into_queue(&mut self) {
	match &self.audio_source {
	    Some(arcit) => {
		let mut guard = arcit.lock().unwrap();
		guard.deref_mut().next(&mut self.queue);
	    },
	    None => {},
	}
	// if self.audio_source.is_some() {
	//     self.audio_source.as_().lock().unwrap().deref_mut().next(&mut self.queue);
	// }
    }

    fn must_report_timeslice(&self) -> bool {
	match self.timeslice {
	    Some(TimesliceUpdate{ update_propagated : false, .. }) => { true }
	    _                                                      => { false },
	}
    }

    /// Returns whether there was progress
    fn update_state_from_next_queue_items(&mut self) {
	if self.queue.len() == 0 {
	    self.poll_iterator_into_queue();
	}
	let action = self.queue.pop_front();
	pinfo!("[ISeq]  ::update: {action:?}");
	match action {
	    Some(AQOp::WaitMillis(0))      => { },
	    Some(AQOp::WaitMillis(millis)) => { self.samples_until_next_poll = (millis * self.target_freq + 500) / 1000 },
	    Some(AQOp::Timeslice(tslice))  => { self.timeslice = TimesliceUpdate::new(tslice); },
	    Some(AQOp::SetSamples(svec))   => { self.set_sample_vec(svec); },
	    Some(AQOp::SetFreq(freq))      => { self.play_freq = freq;
						if self.current_sample.get_freq() != freq {
						    self.push_back_current_sample(false);
						} },
	    Some(AQOp::SetVolume(vol))     => { self.play_volume = vol; },
	    Some(AQOp::End)                => { self.audio_source = None; },
	    None                           => { },
	}
    }

    /// Tries to ensure that self.current_sample is not empty
    fn update_sample_if_needed(&mut self) {
	while self.current_sample.done() && !self.current_sample_vec.is_empty() {
	    let mut offset = None;
	    self.current_sample_range = match self.current_sample_vec.pop_front() {
		Some(AQSample::Once(range)) => Some(range),
		Some(AQSample::OnceAtOffset(range, Some(off)))
		    => { offset = Some(off);
			 Some(range) },
		Some(AQSample::OnceAtOffset(_, None))
		    => panic!("Unexpected"),
		Some(AQSample::Loop(range)) => { self.current_sample_vec.push_front(AQSample::Loop(range));
						 Some(range) },
		None                        => None,
	    };
	    if let Some(range) = self.current_sample_range {
		ptrace!("[ISeq] Requesting sample {range:?} at freq {}", self.play_freq);
		self.current_sample = self.sample_source.borrow_mut().get_sample(range, self.play_freq);
		match offset {
		    Some((off_nom, off_denom)) => self.current_sample.forward_to_offset(off_nom, off_denom),
		    _                          => {},
		}
		if self.current_sample.len() == 0 {
		    panic!("New sample has length 0 -> cannot progress!");
		}
	    }
	}
    }

    /// Polls the iterator until we have reached a ready state.
    fn ensure_ready_state(&mut self) -> ReadyState {
	let mut count = 0;
	loop {
	    if count > 1000 {
		panic!("Stuck?");
	    }
	    count += 1;
	    match self.get_ready_state() {
		Some(s) => { return s; },
		None    => {},
	    }

	    // Timeslices must be enabled through a separate API call to advance_sync(), so only query
	    // if we are waiting for that
	    if self.timeslice.is_none() {
		if self.samples_until_next_poll == 0 {
		    self.update_state_from_next_queue_items();
		}
	    } else {
		self.samples_until_next_poll = self.target_freq; // Unlimited writes
	    }
	    if self.samples_until_next_poll > 0 {
		// Update sample once we know we're going to play this one
		// This avoids requesting a sample at a frequency that we never need to play it at
		self.update_sample_if_needed();
	    }
	}
    }

    /// Gets the current 'ready state', if any, or None otherwise.
    /// No side effects.
    fn get_ready_state(&self) -> Option<ReadyState> {
	if self.flush_requested {
	    return Some(ReadyState::Flush);
	}

	// No active iterator?
	if self.audio_source.is_none() {
	    return Some(ReadyState::ContinuingSilence);
	}

	// Must report timeslice now?
	match self.timeslice {
	    Some(TimesliceUpdate{ timeslice, update_propagated : false }) => { return Some(ReadyState::ReportTimeslice(timeslice)); },
	    _                                                             => {},
	}

	// Otherwise we should have an active iterator and can either play sound or silence.
	let time = self.samples_until_next_poll;
	if time == 0 {
	    return None; // Not ready
	}

	if self.no_sample() {
	    return Some(ReadyState::TemporarySilence(time));
	} else {
	    if self.current_sample.done() {
		if self.current_sample_vec.is_empty() {
		    return Some(ReadyState::TemporarySilence(time));
		} else {
		    return None;
		}
	    }
	    return Some(ReadyState::PlaySample(time));
	}
    }

    /// Stop the current sample and re-request it from the sample source
    /// - If restart_sample = true:  also restart the sample
    /// - If restart_sample = false: maintain same (relative) position
    fn push_back_current_sample(&mut self, restart_sample : bool) {
	match self.current_sample_range {
	    None => {},
	    Some(samplerange) => {
		if restart_sample {
		    self.current_sample_vec.push_front(AQSample::Once(samplerange));
		} else {
		    self.current_sample_vec.push_front(AQSample::OnceAtOffset(samplerange, Some((self.current_sample.get_offset(), self.current_sample.len()))));
		}
		self.stop_sample();
	    },
	}
    }

    fn set_sample_vec(&mut self, svec : Vec<AQSample>) {
	self.current_sample_vec.clear();
	for x in svec {
	    if let AQSample::OnceAtOffset(samplerange, None) = x {
		self.current_sample_vec.push_back(AQSample::OnceAtOffset(samplerange, Some((self.current_sample.get_offset(), self.current_sample.len()))));
	    } else {
		self.current_sample_vec.push_back(x);
	    }
	}
	self.stop_sample();
    }

    fn stop_sample(&mut self) {
	self.current_sample = SampleWriter::empty();
    }

    fn no_sample(&self) -> bool {
	return self.current_sample.is_empty();
    }

    fn waiting_for_next_timeslice(&self) -> bool {
	return self.timeslice.is_some();
    }

    fn success(&mut self, written : usize) -> SyncPCMResult {
	self.timeslice.as_mut().map(|x| x.mark_propagated() );
	ptrace!("-------> Wrote(({written}, {:?})); now: must report={} (should be false)", self.timeslice.as_ref().map(|x| x.timeslice),
		self.must_report_timeslice());
	return SyncPCMResult::Wrote(written, self.timeslice.as_ref().map(|x| x.timeslice));
    }

    fn have_reported_timeslice_update(&self) -> bool {
	return self.timeslice.as_ref().map_or_else(|| false, |x| x.update_propagated);
    }

    fn newly_at_timeslice_boundary(&self) -> bool {
	return self.waiting_for_next_timeslice() && !self.have_reported_timeslice_update();
    }

    // fn have_bounded_time(&self) -> bool {
    // 	return !self.have_reported_timeslice_update();
    // }

    fn record_completed_samples(&mut self, c : usize) {
	pinfo!("[ISeq] ++ progress {c} / {}", self.samples_until_next_poll);
	self.samples_until_next_poll -= c;
    }

    // ----------------------------------------
    // Output operations

    fn write_silence(&mut self, out : &mut [f32]) -> usize {
	out.fill(0.0);
	pdebug!("[ISeq] Filling with silence: {}", out.len());
	return out.len();
    }

    fn write_sample(&mut self, out : &mut [f32]) -> usize {
	let num_samples = usize::min(self.current_sample.remaining(), out.len());
	pdebug!("[ISeq] Asking sample with {} (done={}) to write {num_samples} samples for target len {}",
		self.current_sample.remaining(), self.current_sample.done(), out.len());
	self.current_sample.write(&mut out[..num_samples]);
	if self.play_volume != 1.0 {
	    let vol = self.play_volume;
	    for x in out[..num_samples].iter_mut() {
		*x *= vol;
	    }
	}
	pdebug!("[ISeq] Writing {num_samples} samples");
	return num_samples;
    }
}

impl FrequencyTrait for IteratorSequencer {
    fn frequency(&self) -> Freq {
	return self.target_freq;
    }
}

impl PCMSyncWriter for IteratorSequencer {
    fn write_sync_pcm(&mut self, outbuf : &mut [f32]) -> SyncPCMResult {
	let mut outbuf_pos = 0;
	let outbuf_len = outbuf.len();
	pdebug!("[ISeq] Asked for {outbuf_len} samples");

	while outbuf_pos < outbuf_len {
	    pdebug!("Completed {outbuf_pos} / {outbuf_len}");
	    let mut out = &mut outbuf[outbuf_pos..];
	    let outlen = out.len();

	    let ready_state = self.ensure_ready_state();
	    pdebug!("[ISeq] {ready_state:?} for {} samples", self.samples_until_next_poll);
	    let completed = match ready_state {
		ReadyState::Flush               => { self.flush_requested = false;
						     pdebug!("[ISeq] => Flush");
						     return SyncPCMResult::Flush; },
		ReadyState::ContinuingSilence   => { self.samples_until_next_poll = 0;
						     self.write_silence(&mut out) },
		ReadyState::TemporarySilence(t) => self.write_silence(&mut out[..usize::min(t, outlen)]),
		ReadyState::PlaySample(t)       => self.write_sample(&mut out[..usize::min(t, outlen)]),
		ReadyState::ReportTimeslice(_)  => { break; },
	    };
	    if ready_state != ReadyState::ContinuingSilence {
		self.record_completed_samples(completed);
	    }
	    outbuf_pos += completed;
	}
	pdebug!("[ISeq] => Success with {outbuf_pos} samples");
	return self.success(outbuf_pos);
    }

    fn advance_sync(&mut self, timeslice : super::dsp::writer::Timeslice) {
	assert!(self.timeslice.is_some());
	assert_eq!(self.timeslice.as_ref().unwrap().timeslice, timeslice);
	self.timeslice = None;
	self.samples_until_next_poll = 0;
	self.timeslice = None;
    }

}

impl AudioIteratorProcessor for IteratorSequencer {
    // Finishes playing the current sample
    fn flush(&mut self) {
	self.hard_reset();
	self.flush_requested = true;
    }

    fn set_source(&mut self, source : ArcIt) {
	// commented out: stopping the current sample will probably not yield good results
	//self.current_sample = SampleWriter::empty();
	self.audio_source = Some(source);
	self.soft_reset();
	pinfo!("[ISeq] ** New iterator installed -> {:?} samples remain ({} / {}))",
	       self.samples_until_next_poll,
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
/// testdata : (pos, len, freq, out_start, out_len)
fn setup_samplesource(testdata : Vec<(usize, usize, usize, f32, usize)>) -> Rc<RefCell<SincSampleSource>> {
    let mut v = vec![];
    for (pos, len, freq, out_start, out_len) in testdata {
	let mut data = vec![];
	for n in 0..out_len {
	    data.push(out_start + (n as f32));
	}
	v.push(((pos, len, freq), data));
    }
    return Rc::new(RefCell::new(SincSampleSource::nw(1, v)));
}

// ----------------------------------------
// Tests

#[cfg(test)]
#[test]
fn test_default_silence() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(100),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource(vec![]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_sample_bufsize_limited() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource(vec![(0, 10, 1000, 1.0, 10)]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
	       &outbuf[..]);
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
    let ssrc = setup_samplesource(vec![
	(0, 5, 1000, 1.0, 5),
	(10, 20, 1000, 11.0, 20),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_sample_loop() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(0,2)),
				       ]),
				       AQOp::WaitMillis(20)]]);
    let ssrc = setup_samplesource(vec![(0, 2, 1000, 1.0, 2)]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 1.0, 2.0, 1.0, 2.0, 1.0, 2.0],
	       &outbuf[..]);
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
    let ssrc = setup_samplesource(vec![
	(0, 3, 1000, 1.0, 3),
	(10, 2, 1000, 11.0, 2),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([11.0, 12.0, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0],
	       &outbuf[..]);
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
    let ssrc = setup_samplesource(vec![
	(10, 2, 1000,
	 11.0, 2),
	(20, 1, 1000,
	 21.0, 1),
	(0, 2, 1000,
	 1.0, 2),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([11.0, 12.0, 21.0, 1.0, 2.0, 1.0, 2.0, 1.0],
	       &outbuf[..]);
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
    let ssrc = setup_samplesource(vec![
	(0, 2, 1000,  1.0, 2),
	(10, 4, 2000, 11.0, 4),
	(10, 4, 500, 111.0, 4),
	(20, 10, 500, 21.0, 20),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    // expect hard switches
    assert_eq!([1.0, 2.0, 11.0, 112.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0],
	       &outbuf[..]);
    assert_eq!(SyncPCMResult::Wrote(10, None), r);
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
    let ssrc = setup_samplesource(vec![
	(0, 20, 1000, 1.0, 20),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([10.0, 20.0, 3.0, 4.0, 10.0, 12.0, 14.0, 16.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_run_out() {
    let mut outbuf0 = [-1.0; 8];
    let mut outbuf1 = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(5),
				       AQOp::End,
    ]]);

    let ssrc = setup_samplesource(vec![
	(0, 3, 1000, 1.0, 3),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf0));
    let r = iseq.write_sync_pcm(&mut outbuf0);

    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf0[..]);
    let r = iseq.write_sync_pcm(&mut outbuf1);
    assert_eq!(SyncPCMResult::Wrote(8, None), r);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf1[..]);
}

#[cfg(test)]
#[test]
fn test_replace_iterator() {
    let mut outbuf = [-1.0; 8];
    let ait = iterator::mock(vec![vec![AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(10,4))]),
				       AQOp::WaitMillis(1000)]]);
    let ssrc = setup_samplesource(vec![
	(0, 10, 1000, 1.0, 10),
	(10, 4, 2000, 11.0, 4),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf[0..2]);

    assert_eq!(SyncPCMResult::Wrote(2, None), r);
    assert_eq!([11.0, 12.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..]);

    let ait2 = iterator::mock(vec![vec![AQOp::SetFreq(1000),
					AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,10))]),
					AQOp::WaitMillis(1000)]]);
    iseq.set_source(ait2);
    let r = iseq.write_sync_pcm(&mut outbuf[2..]);
    assert_eq!(SyncPCMResult::Flush, r);
    let r = iseq.write_sync_pcm(&mut outbuf[2..]);

    assert_eq!([11.0, 12.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
	       &outbuf[..]);
    assert_eq!(SyncPCMResult::Wrote(6, None), r);
}

#[cfg(test)]
#[test]
fn test_sample_silence_in_between() {
    let mut outbuf = [-1.0; 12];

    let ait = iterator::mock(vec![vec![AQOp::SetFreq(1000),
				       AQOp::SetSamples(vec![AQSample::Once(SampleRange::new(0,3))]),
				       AQOp::WaitMillis(2),
				       AQOp::SetVolume(100.0),
				       AQOp::WaitMillis(2),
				       AQOp::SetFreq(2000),
				       AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(10,3))]),
				       AQOp::WaitMillis(10),
    ]]);
    let ssrc = setup_samplesource(vec![
	(0, 3, 1000, 1.0, 3),
	(0, 3, 2000, 111111.0, 3),
	(10, 3, 2000, 11.0, 3),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!([1.0, 2.0, 300.0, 0.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0],
	       &outbuf[..]);
    assert_eq!(SyncPCMResult::Wrote(12, None), r);
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
				       AQOp::WaitMillis(10),
    ]]);
    let ssrc = setup_samplesource(vec![
	(0, 3, 1000, 1.0, 3),
	(0, 3, 2000, 111111.0, 3),
	(10, 3, 2000, 11.0, 3),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf);

    assert_eq!([1.0, 2.0, 300.0, 100.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0, 1300.0, 1100.0, 1200.0],
	       &outbuf[..]);
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
				       AQOp::WaitMillis(1),
				       AQOp::WaitMillis(1),
				       AQOp::WaitMillis(1),
				       AQOp::Timeslice(3),
    ]]);
    let ssrc = setup_samplesource(vec![
	(0, 3, 1000, 1.0, 3),
	(10, 3, 2000, 11.0, 3),
    ]);
    let mut iseq = IteratorSequencer::nw(ait, ssrc);
    assert_eq!(SyncPCMResult::Flush, iseq.write_sync_pcm(&mut outbuf));
    let r = iseq.write_sync_pcm(&mut outbuf[0..1]);
    assert_eq!(SyncPCMResult::Wrote(1, None), r);
    let r = iseq.write_sync_pcm(&mut outbuf[1..5]);
    assert_eq!(SyncPCMResult::Wrote(1, Some(1)), r);
    let r = iseq.write_sync_pcm(&mut outbuf[2..5]);
    assert_eq!(SyncPCMResult::Wrote(3, Some(1)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		-1.0, -1.0, -1.0, -1.0, -1.0],
	       &outbuf[..10]);

    iseq.advance_sync(1);

    let r = iseq.write_sync_pcm(&mut outbuf[5..]);
    assert_eq!(SyncPCMResult::Wrote(2, Some(2)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		3000.0, 11.0,
		// ts-2 available
		-1.0, -1.0, -1.0],
	       &outbuf[..10]);

    let r = iseq.write_sync_pcm(&mut outbuf[7..10]);
    assert_eq!(SyncPCMResult::Wrote(3, Some(2)), r);

    assert_eq!([1.0, 2.0,
		// ts-1 available
		300.0, 100.0, 200.0,
		// ts-1 active
		3000.0, 11.0,
		// ts-2 available
		12.0, 13.0, 11.0,
		-1.0],
	       &outbuf[..11]);

    iseq.advance_sync(2);

    let r = iseq.write_sync_pcm(&mut outbuf[10..]);
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

}
