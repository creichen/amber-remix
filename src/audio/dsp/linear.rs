// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

/// Linearly interpolating remixer
///
/// Not expected to produce particularly high-quality output.  The main benefit of this mixer
/// is that it can "resample" between arbitrary frequencies and produce something that is mildly
/// better than the bare minimum.


use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

use crate::audio::dsp::vtracker;

use crate::audio::dsp::writer::PCMSyncWriter;
use crate::audio::dsp::writer::PCMFlexWriter;
use crate::audio::dsp::writer::SyncPCMResult;
use crate::audio::dsp::ringbuf::RingBuf;
use crate::util::IndexLen;
use super::frequency_range::Freq;
use super::frequency_range::FreqRange;
use super::vtracker::TrackerSensor;
use super::writer::FrequencyTrait;
use super::writer::Timeslice;

const BUFFER_SIZE_MILLIS : usize = 500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct TimesliceGuard {
    timeslice : Timeslice,
    reported : bool, // Reported at least once to caller? Otherwise we can't query source for more data yet.
    samples_until_timeslice : usize, // Slices in "buf" that we can process without hitting the timeslice barrier
}

pub struct LinearFilter {
    // const
    output_freq : Freq,
    source : Rc<RefCell<dyn PCMFlexWriter>>,
    tracker : TrackerSensor,

    // Input state
    buf : RingBuf,
    freqs : FreqRange,
    timeslice : Option<TimesliceGuard>,

    // Resampler state
    resampler : Option<SampleState>,
}

impl LinearFilter {
    #[cfg(test)]
    fn nw(max_in_freq : Freq, out_freq : Freq, source : Rc<RefCell<dyn PCMFlexWriter>>) -> LinearFilter {
	return LinearFilter::new(max_in_freq, out_freq, source, TrackerSensor::new());
    }

    pub fn new(max_in_freq : Freq, out_freq : Freq, source : Rc<RefCell<dyn PCMFlexWriter>>, tracker : TrackerSensor) -> LinearFilter {
	return LinearFilter {
	    output_freq: out_freq,
	    source,
	    tracker,
	    buf : RingBuf::new(max_in_freq * BUFFER_SIZE_MILLIS / 1000),
	    freqs : FreqRange::new(),
	    timeslice : None,
	    resampler : None,
	};
    }

    pub fn get_timeslice(&self) -> Option<Timeslice> {
	if let Some(TimesliceGuard { samples_until_timeslice : 0, timeslice, .. }) = self.timeslice {
	    return Some(timeslice);
	}
	return None;
    }

    pub fn samples_until_timeslice(&self) -> Option<usize> {
	if let Some(TimesliceGuard { samples_until_timeslice, reported : false, .. }) = self.timeslice {
	    return Some(samples_until_timeslice);
	}
	return None;
    }

    pub fn timeslice_locked(&self) -> bool {
	return self.samples_until_timeslice() == Some(0);
    }

    pub fn unlock_timeslice(&mut self) {
	if let Some(TimesliceGuard { samples_until_timeslice : 0, reported : false, timeslice }) = self.timeslice {
	    self.timeslice = Some(TimesliceGuard {
		samples_until_timeslice : 0,
		reported : true,
		timeslice
	    });
	}
    }

    /// Request data from the source to fill the local buffer
    /// If STATUS_CHECK is set, we only check for status updates
    fn fill_local_buffer(&mut self, status_check : bool) -> SyncPCMResult {
	if self.timeslice_locked() {
	    return SyncPCMResult::Wrote(0, self.get_timeslice());
	}
	let mut max_to_write = if status_check { 0 } else { self.buf.remaining_capacity() };
	if None != self.get_timeslice() {
	    // heuristic: don't fill the buffer with data we're likely to discard
	    max_to_write = usize::min(max_to_write, 64);
	}
	let offered_to_write;

        let write_result = {
	    let mut freqs_at_buf_offset = self.freqs.at_offset(self.buf.len());
	    let mut buf = self.buf.wrbuf(max_to_write);
	    offered_to_write = buf.len();
	    self.source.borrow_mut().write_flex_pcm(&mut buf, &mut freqs_at_buf_offset)
	};

	if let SyncPCMResult::Wrote(num_written, timeslice) = write_result {
	    self.buf.drop_back(offered_to_write - num_written).unwrap();
            if num_written == 0 && !status_check {
		let no_timeslice_reported = timeslice == None;
		let timeslice_already_known = self.timeslice != None;
		if no_timeslice_reported || timeslice_already_known {
		    perror!("Buffer error, status: {max_to_write}/{offered_to_write}/{max_to_write}/{}/{}", self.buf.len(), self.buf.capacity());
		    if self.buf.is_full() {
			panic!("Buffer full");
		    } else if offered_to_write == 0 {
			panic!("RingBuf had capacity {} but offered no write buffer", max_to_write);
		    } else {
			panic!("Source to LinearFilter refused to provide updates (offered {offered_to_write})");
		    }
		}
	    }

	    ptrace!("** prep: wrote {num_written}/{max_to_write}, now have {}", self.buf.len());
	}

	return write_result;
    }

    fn max_available_to_read(&self) -> usize {
	let mut available = self.buf.len();
	if self.timeslice_locked() {
	    if let Some(ts) = self.timeslice {
		available = ts.samples_until_timeslice;
	    }
	}
	return available;
    }

    fn skip_input_sample(&mut self) {
	if let (_, Some(remaining)) = self.freqs.get(0) {
	    self.advance_input(remaining);
	    return;
	}
	if let Some(remaining) = self.samples_until_timeslice() {
	    self.advance_input(remaining);
	    return;
	}
	panic!("Trying to skip input sample even though we don't know its end yet");
    }

    fn advance_input(&mut self, len : usize) {
	self.buf.drop_front(len).unwrap();
	self.freqs.shift(len);
	if let Some(TimesliceGuard { samples_until_timeslice, reported, timeslice }) = self.timeslice {
	    if samples_until_timeslice > 0 {
		self.timeslice = Some(TimesliceGuard {
		    // Provoke underflow error if we cross this threshold by accident:
		    samples_until_timeslice : samples_until_timeslice - len,
		    reported,
		    timeslice
		});
	    }
	}
    }

    fn get_resampler(&mut self, in_freq: usize) -> SampleState {
	let time_into_resampler = 0.0;
	match self.resampler {
	    // Frequency change?  Invalidate.
	    Some(s) => { if s.in_freq == in_freq {
			    return s;
	               } else {
//		            time_into_resampler = f32::from(s);
	    }},
	    None    =>  {},
	};
	// Heuristic: favour actual samples unless we've already made some progress
	// if time_into_resampler <= 0.5 {
	//    time_into_resampler = 0.0;
	// }
	pdebug!("Updating sample conversion rate: {in_freq} Hz => {} Hz, start={time_into_resampler} ", self.output_freq);
	let result = SampleState::new(in_freq, self.output_freq, time_into_resampler);
	self.resampler = Some(result);
	return result;
    }

    fn resample_to_output(&mut self, resampler : &mut SampleState, output_slice : &mut [f32], max_read : usize) -> usize {
	pdebug!("        --> Requesting resampler with [0..{}] <-  [0..{}] (len={})", output_slice.len(), max_read, self.buf.len());
	resampler.resample(output_slice,
			   self.buf.peek_front(max_read));
	let samples_used = resampler.reset_int_position(self.buf.len());
	self.advance_input(samples_used);
	return samples_used;
    }

    fn emit_buffer(&mut self, output: &mut [f32]) -> usize {
        let out_end = output.len();
        let mut out_pos = 0;
        'pull_loop: while out_pos < out_end && !self.timeslice_locked() {
	    let out_remaining = out_end - out_pos;
	    let in_remaining = self.max_available_to_read();

	    ptrace!("... onto the next; in: buf_len:{}, left:{}, timeslice:{:?}",
			     self.buf.len(), in_remaining, self.timeslice);

	    if self.freqs.is_empty() {
		// Freqs tell us the frequency of each input sample.  If this is empty while we
		// still have samples, there's a bug in the source or in the code that propagates
		// data from the source.
		panic!("Buffer input = {} output = [{out_pos}..{out_end}] but freqs = {}", self.buf.len(), self.freqs);
	    }

	    // How much sample information can we read now?
	    let (insample_freq, insample_remaining) = self.freqs.get(0);
	    let (mut insample_num_available, insample_is_ending) = match insample_remaining {
		None    => (in_remaining,
			    // input sample is ending in this case iff we can hit the timeslice
			    Some(in_remaining) == self.samples_until_timeslice()),
		Some(l) => (usize::min(l, in_remaining),
		            // input sample is ending in this case iff we have all of its data
			    l <= in_remaining),
	    };
	    //let num_samples_in_per_out = in_freq as f32 / self.output_freq as f32;

	    // Make sure we have the linear remixer set up
	    let mut resampler = self.get_resampler(insample_freq);

	    // Heuristic: try to oversupply resampler
	    if insample_num_available + 1 > in_remaining {
		// We lack that extra sample for nicer interpolation?
		if None == self.timeslice {
		    // No timelice yet => source has more to send
		    if !self.buf.is_full() {
			// ...and we have the space => let's stop here and ask for more data first
			pdebug!("   let's grab more data");
			break 'pull_loop;
		    }
		    // ...but we don't have storage => clear up some of that storage
		    insample_num_available = (insample_num_available / 2) + 1
		}
	    };

	    // How much sample information can we write now?
	    let max_out_from_insample = resampler.max_out_possible(insample_num_available);
	    if resampler.sample_pos_int > self.buf.len() {
		pwarn!("Suspicious buflen for {} vs {}, culling", resampler, self.buf.len());
		resampler.sample_pos_int = 0;
		self.resampler = Some(resampler);
		continue;
	    }

	    // Try to add an extra byte to improve interpolation
	    let max_from_insample = usize::min(insample_num_available + 1,
					       self.buf.len());

	    // Input is empty?
	    if max_from_insample == 0 {
		pinfo!("Input buffer suddenly empty, must request additional data");
		break;
	    }

	    // Can we make any progress?
	    if max_out_from_insample == 0 {
		pdebug!("Can't progress: max_out = 0");
		if insample_is_ending {
		    // Current example is no longer useful, discard it and move on to the next
		    self.skip_input_sample();
		    pdebug!("   input sample ending in {} and won't be useful any longer, skip (timeslice lock now: {})",
				     insample_num_available, self.timeslice_locked());
		    continue;
		}
		// possible alternative causes:
		// - we don't have enough data for the current sample -> can poll for more
		pdebug!("   should for more data");
		break;
	    }

	    let max_out = usize::min(out_end - out_pos,
				     max_out_from_insample);

	    ptrace!("-- out@{out_pos}");
	    ptrace!("   freqs={}", self.freqs);
	    ptrace!("   outbuf=[{out_pos}..{out_end}]  -> len={out_remaining}");
	    ptrace!("   inbuf=[0..{:?}]", self.buf.len());
	    ptrace!("     -> expected max-out={max_out} = min({}, {max_out_from_insample})", out_end - out_pos);
	    ptrace!("        expected max inbuf read: [0..{}] (from max {:?})", insample_num_available, insample_remaining);
	    ptrace!("        it: {resampler}");

	    let read = self.resample_to_output(&mut resampler, &mut output[out_pos..out_pos+max_out],
					       max_from_insample);

	    if vtracker::ENABLED {
		for x in &output[out_pos..out_pos+max_out] {
		    self.tracker.add(*x);
		}
	    }

	    out_pos += max_out;
	    ptrace!("        read: {read}");
	    ptrace!("        it': {resampler}");
	    self.resampler = Some(resampler);
	}
        return out_pos;
    }

    pub fn return_success(&mut self, output_written : usize) -> SyncPCMResult {
	self.unlock_timeslice();
	return SyncPCMResult::Wrote(output_written, self.get_timeslice());
    }
}

impl FrequencyTrait for LinearFilter {
    fn frequency(&self) -> Freq {
	return self.output_freq;
    }
}

impl PCMSyncWriter for LinearFilter {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
	let output_requested = output.len();
	let mut output_written = 0;
	let mut first_iteration = true;
	while output_written < output_requested {
	    #[allow(unused)]
	    let mut num_read = 0;
	    loop {
		match self.fill_local_buffer(first_iteration && !self.buf.is_empty()) {
		    SyncPCMResult::Wrote(r, timeslice) => {
			num_read = r;
			if self.timeslice == None {
			    if let Some(timeslice) = timeslice {
				pdebug!("[TOP]  :: timeslice({timeslice}) at offset {}", self.buf.len());
				self.timeslice = Some(TimesliceGuard {
				    timeslice,
				    reported : false,
				    samples_until_timeslice : self.buf.len(),
				});
			    }
			}
			break;
		    },
		    SyncPCMResult::Flush => {
			self.buf.reset();
			self.timeslice = None;
			self.freqs = FreqRange::new();
			self.resampler = None;
			continue;
		    }
		}
	    };
	    // ptrace!("[TOP]  buf = {:?}", &self.buf[..self.buf.len()]);
	    // ptrace!("[TOP]  out = {:?}", &output[..output_written]);
	    pdebug!("[TOP]  after {num_read} reads: requesting write at: {output_written}/{output_requested} with {}/{} samples", self.buf.len(), self.buf.len());
	    let num_written = self.emit_buffer(&mut output[output_written..]);

	    output_written += num_written;
	    pdebug!("[TOP]  TOTAL PROGRESS: {output_written}/{output_requested} with {} samples", self.buf.len());
	    if num_read == 0 && num_written == 0 && !first_iteration {
		if self.timeslice_locked() {
		    pdebug!("[TOP] Timeslice-locked, so returning");
		    return self.return_success(output_written);
		} else {
		    panic!("No progress in linear filter: input buf {}/{} vs out {output_written}/{output_requested}", self.buf.len(), self.buf.len());
		}
	    }
	    first_iteration = false;
        }
	pdebug!("[TOP] Returning with {output_written}");
	return self.return_success(output_written);
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
	match self.timeslice {
	    Some(ts) => { assert_eq!(ts.timeslice, timeslice); },
	    None     => { panic!("Cannot advance timeslice"); },
	}
	self.timeslice = None;
	self.source.borrow_mut().advance_sync(timeslice);
	// Discard all obsolete data
	self.advance_input(self.buf.len());
    }
}

// ----------------------------------------
// SampleState: The linear interpolator

#[derive(Copy, Clone)]
struct SampleState {
    in_freq : Freq,
    out_freq : Freq,         // output buffer frequency

    // index into sample data
    sample_pos_int : usize,   // integral part
    sample_pos_fract : f32, // fractional part (nominator; the denominator is out_freq)
}

impl SampleState {
    fn new(in_freq : Freq, out_freq : Freq, time_into : f32) -> SampleState {
	let int_samples_into = (time_into * in_freq as f32) as usize;
	SampleState {
	    in_freq,
	    out_freq,
	    sample_pos_int : int_samples_into / out_freq,
	    sample_pos_fract : (int_samples_into % out_freq) as f32,
	}
    }

    /// Output samples we expect to generate for the given number of input samples
    fn max_out_possible(&self, in_samples : usize) -> usize {
	return (self.sample_pos_fract + (in_samples * self.out_freq) as f32) as usize / self.in_freq;
    }

    // Reduce the integral part of the position by up to MAX
    fn reset_int_position(&mut self, max : usize) -> usize {
	let pos = usize::min(max, self.sample_pos_int);
	self.sample_pos_int -= pos;
	return pos;
    }

    fn resample<T>(&mut self, outbuf : &mut [f32], inbuf : T) where T : IndexLen<f32> {
	let inbuf_len = inbuf.len();
	if inbuf_len == 0 {
	    panic!("Cannot resample with buffer size zero.");
	}
	//ptrace!("  ## [..{}] <- [..{}]", outbuf.len(), inbuf.len());
	//ptrace!("  ## resamp from {}", inbuf.get(0));
	let mut pos = self.sample_pos_int;

	// fractional position counter
	let mut fpos_nom = self.sample_pos_fract as f32;
	let fpos_nom_inc_total = self.in_freq;
	let fpos_denom = self.out_freq as f32;
	let pos_inc = (fpos_nom_inc_total / self.out_freq) as usize;
	let fpos_nom_inc = (fpos_nom_inc_total % self.out_freq) as f32;

	for out in outbuf.iter_mut() {
	    //ptrace!("  ## out <- in[{}]", pos);
	    // Linear interpolation
	    let safe_pos = usize::min(pos, inbuf_len - 1);
	    let sample_v_current = inbuf.get(safe_pos);

	    let sample_v_next = if safe_pos + 1 == inbuf_len  { sample_v_current } else { inbuf.get(safe_pos + 1) };

	    let sample_v_current_fragment = sample_v_current * (fpos_denom - fpos_nom);
	    let sample_v_next_fragment = sample_v_next * fpos_nom;

	    //ptrace!("  ## interpol {}, {}", sample_v_current, sample_v_next);

	    let sample_v = (sample_v_current_fragment + sample_v_next_fragment) / fpos_denom;

	    //ptrace!("  ## => {sample_v}");
	    *out = sample_v;

	    pos += pos_inc;
	    fpos_nom = fpos_nom + fpos_nom_inc;
	    if fpos_nom >= fpos_denom {
		fpos_nom -= fpos_denom;
		pos += 1;
	    }
	}
	self.sample_pos_int = pos;
	self.sample_pos_fract = fpos_nom;
    }
}

impl Display for SampleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	write!(f, "SampleState{{ in:{} Hz -> out:{} Hz; pos : {} + {}/{} }}",
	       self.in_freq, self.out_freq,
	       self.sample_pos_int, self.sample_pos_fract, self.out_freq)
    }
}

impl From<SampleState> for f32 {
    fn from(s: SampleState) -> Self {
	((s.sample_pos_int * s.in_freq) as f32 + s.sample_pos_fract) / s.out_freq as f32
    }
}

// ========================================
// Testing

#[cfg(test)]
use crate::audio::dsp::pcmsync::{PCMBasicSyncBarrier, T, mock_asw, cread};
#[cfg(test)]
use crate::audio::dsp::writer::PCMSyncBarrier;
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use super::pcmsync;


// ----------------------------------------
// Helpers

#[cfg(test)]
struct MFWTick {
    maxwrite : usize,
    s : Vec<f32>,
    f : Vec<(usize, Freq)>,
    timeslice : usize,
}

#[cfg(test)]
impl MFWTick {
    fn new(samples : Vec<isize>, freqs : Vec<(usize, Freq)>) -> MFWTick {
	return MFWTick::new_with_maxwrite(1000, samples, freqs);
    }

    fn new_with_maxwrite(maxwrite : usize, samples : Vec<isize>, freqs : Vec<(usize, Freq)>) -> MFWTick {
	let mut s_f32 = Vec::new();
	for s in samples {
	    s_f32.push(s as f32);
	}
	return MFWTick {
	    maxwrite,
	    s : s_f32,
	    f : freqs,
	    timeslice : 0,
	}
    }

    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange) -> SyncPCMResult {
	let maxsize = usize::min(self.maxwrite,
				 usize::min(output.len(), self.s.len()));
	if self.is_empty() {
	    output.fill(-1.0 * self.timeslice as f32);
	    return SyncPCMResult::Wrote(output.len(), Some(self.timeslice));
	}
	output[0..maxsize].copy_from_slice(&self.s[0..maxsize]);
	let f = &self.f;
	for (pos, freq) in f {
	    freqrange.append(*pos, *freq);
	}
	self.f = vec![];
	self.s.copy_within(maxsize.., 0);
	self.s.truncate(self.s.len() - maxsize);
	let ts = if self.is_empty() { Some(self.timeslice) } else { None };
	return SyncPCMResult::Wrote(maxsize, ts);
    }

    fn is_empty(&self) -> bool {
	return self.s.is_empty();
    }
}

#[cfg(test)]
struct MockFlexWriter {
    t : VecDeque<RefCell<MFWTick>>,
    ticks : Option<usize>,
}

#[cfg(test)]
impl PCMFlexWriter for MockFlexWriter {
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange) -> SyncPCMResult {
	match self.t.front() {
	    None => { panic!("Out of slices"); },
	    Some(t) => {
		let result = t.borrow_mut().write_flex_pcm(output, freqrange);
		match result {
		    SyncPCMResult::Wrote(_, Some(i)) => { self.ticks = Some(i);  },
		    _                                => {},
		}
		println!("[MFWTick] {:?} (requested {})", result, output.len());
		return result;
	    }
	}
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
	assert_eq!(Some(timeslice), self.ticks);
	self.ticks = None;
	self.t.pop_front();
    }
}

#[cfg(test)]
impl MockFlexWriter {
    pub fn new(t : Vec<MFWTick>) -> MockFlexWriter {
	let mut tdeque = VecDeque::new();
	for tt in &t {
	    tdeque.push_back(RefCell::new(MFWTick{
		maxwrite : tt.maxwrite,
		s : (&tt.s[..]).to_vec(),
		f : (&tt.f[..]).to_vec(),
		timeslice : 0,
	    }));
	}
	for (index, tt) in tdeque.iter_mut().enumerate() {
	    tt.borrow_mut().timeslice = index + 1;
	}
	MockFlexWriter {
	    t : tdeque,
	    ticks : None,
	}
    }
}

// ----------------------------------------
// Tests

#[cfg(test)]
#[test]
fn test_copy() {
    let mut outbuf : [f32; 5] = [0.0; 5];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 100, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 20.0,
		 100.0,
		 10.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_double() {
    let mut outbuf : [f32; 10] = [0.0; 10];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 200, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 12.5,
		 20.0,
		 60.0,
		 100.0,
		 55.0,
		 10.0,
		 25.0,
		 40.0,
		 40.0,],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_triple() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_incremental() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
		 &outbuf[..]);

    sstate.resample(&mut outbuf[2..11], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0 ],
		 &outbuf[..]);
    sstate.resample(&mut outbuf[11..15], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_double() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(100, 50, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..3], &inbuf);
    assert_eq!( [10.0,
		 30.0,
		 50.0,
		 0.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_ten() {
    let mut outbuf : [f32; 10] = [0.0; 10];
    let inbuf = vec![1.0, 2.0, 3.0];
    let mut sstate = SampleState::new(10, 100, 0.0);
    sstate.resample(&mut outbuf[0..10], &inbuf);
    assert_eq!( [1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_one_point_five() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_one_point_five_incremental() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100, 0.0);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 0.0,
		 0.0 ],
		 &outbuf[..]);
    sstate.resample(&mut outbuf[2..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

// --------------------
// Full filter

#[cfg(test)]
#[test]
fn test_linear_filter_limit_writes() {
     for write_size in 1..20 {
	 println!("[test_linear_filter_limit_writes] limit = {write_size}");
 	let mut outbuf = [0.0; 14];
	let flexwriter = MockFlexWriter::new(vec![
	// slice 1
	    MFWTick::new_with_maxwrite(write_size, vec![
		1, 2,                       //   1:1
		3, 4, 5, 6,                 //   2:1 (downsample)
		7, 8, 9,                    //   1:2 (upsample)
		10, 20, 30, 40, 50, 60, 70  // 1.5:1 (downsample)
	    ], vec![(0, 10000), (2, 20000), (6, 5000), (9, 15000)]),
	]);

	let mut lf = LinearFilter::nw(20000, 10000, Rc::new(RefCell::new(flexwriter)));
	assert_eq!(SyncPCMResult::Wrote(14, None), lf.write_sync_pcm(&mut outbuf[..]));
	assert_eq!( [1.0, 2.0,
		     3.0, 5.0,
		     7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		     10.0, 25.0, 40.0, 55.0,],
		     &outbuf[..]);
    }
}

#[cfg(test)]
#[test]
fn test_linear_filter_resampling_incremental() {
    let mut outbuf = [0.0; 14];
    let flexwriter = MockFlexWriter::new(vec![
	// slice 1
	MFWTick::new(vec![
	    1, 2,                       //   1:1
	    3, 4, 5, 6,                 //   2:1 (downsample)
	    7, 8, 9,                    //   1:2 (upsample)
	    10, 20, 30, 40, 50, 60, 70  // 1.5:1 (downsample)
	], vec![(0, 10000), (2, 20000), (6, 5000), (9, 15000)]),
	]);

    let mut lf = LinearFilter::nw(20000, 10000, Rc::new(RefCell::new(flexwriter)));

    assert_eq!(SyncPCMResult::Wrote(1, None), lf.write_sync_pcm(&mut outbuf[0..1]));
    assert_eq!( [1.0,
		 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    assert_eq!(SyncPCMResult::Wrote(3, None), lf.write_sync_pcm(&mut outbuf[1..4]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    assert_eq!(SyncPCMResult::Wrote(1, None), lf.write_sync_pcm(&mut outbuf[4..5]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0,
		 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    assert_eq!(SyncPCMResult::Wrote(1, None), lf.write_sync_pcm(&mut outbuf[5..6]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5,
		 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);
    assert_eq!(SyncPCMResult::Wrote(1, None), lf.write_sync_pcm(&mut outbuf[6..7]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0,
		 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    assert_eq!(SyncPCMResult::Wrote(4, None), lf.write_sync_pcm(&mut outbuf[7..11]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0,
		 0.0, 0.0, 0.0,
		 ],
		 &outbuf[..]);

    assert_eq!(SyncPCMResult::Wrote(3, None), lf.write_sync_pcm(&mut outbuf[11..14]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 // could go with a 6.5 start here...
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0, // ... to bring this one down a notch
		 10.0, 25.0, 40.0, // .. and there is room for imrpovement here, too.
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_linear_filter_multislice() {
    let mut outbuf = [0.0; 17];
    let flexwriter = MockFlexWriter::new(vec![
	// slice 1
	MFWTick::new(vec![
	    1, 2,                       // 1:1
	    3, 4, 5, 6,                 // 2:1 (downsample)
	], vec![(0, 10000), (2, 20000)]),
	// slice 2
	MFWTick::new(vec![
	    7, 8, 9,                    //   1:2 (upsample)
	    10, 20, 30, 40, 50, 60, 70  // 1.5:1 (downsample)
	], vec![(0, 5000), (3, 15000)]),
	]);

    let mut lf = LinearFilter::nw(20000, 10000, Rc::new(RefCell::new(flexwriter)));
    assert_eq!(SyncPCMResult::Wrote(4, Some(1)), lf.write_sync_pcm(&mut outbuf[..]));
    assert_eq!(SyncPCMResult::Wrote(2, Some(1)), lf.write_sync_pcm(&mut outbuf[4..6]));
    assert_eq!(SyncPCMResult::Wrote(1, Some(1)), lf.write_sync_pcm(&mut outbuf[6..7]));
    lf.advance_sync(1);
    assert_eq!(SyncPCMResult::Wrote(10, None), lf.write_sync_pcm(&mut outbuf[7..]));
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 -1.0, -1.0, -1.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0, 25.0, 40.0, 55.0,
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_linear_filter_boundary_crash() {
    let mut outbuf = [0.0; 17];
    let flexwriter = MockFlexWriter::new(vec![
	// slice 1
	MFWTick::new(vec![
	    1, 2, 3, 4, 5, 6,           // 1:0.95

	], vec![(0, 9500)]),
	// slice 2
	MFWTick::new(vec![
	    7, 8, 9,                    //   1:2 (upsample)
	    10, 20, 30, 40, 50, 60, 70  // 1.5:1 (downsample)
	], vec![(0, 5000), (3, 15000)]),
	]);

    let mut lf = LinearFilter::nw(20000, 10000, Rc::new(RefCell::new(flexwriter)));
    assert_eq!(SyncPCMResult::Wrote(3, None), lf.write_sync_pcm(&mut outbuf[..3]));
    assert_eq!(SyncPCMResult::Wrote(5, Some(1)), lf.write_sync_pcm(&mut outbuf[3..]));
    assert_eq!( [1.0, 1.95,
		 2.9, 3.85,
		 4.8, 5.75,
		 6.0, 6.0
		 ],
		 &outbuf[..8]);
}

// ----------------------------------------
// Syonchronisation integration tests

#[cfg(test)]
#[test]
fn integrate_test_binary_sync() {

    let mut data0 = [0.0; 20];
    let mut data1 = [0.0; 20];
    let mut sbar = PCMBasicSyncBarrier::new();

    let c0 = sbar.sync(mock_asw("0".to_string(), vec![
	T::S(vec![10.0, 11.0]),
	T::TS(-11.0, 1),
	T::S(vec![80.0, 81.0, 82.0, 83.0, 84.0, 85.0, 86.0, 07.0, 88.0, 09.0]),
	T::TS(-12.0, 2),
	T::S(vec![14.0, 15.0, 16.0, 17.0]),
	T::TS(-13.0, 3),
    ]));
    let flexwriter = MockFlexWriter::new(vec![
	// slice 1
	MFWTick::new(vec![
	    1, 2,                   // 1:1
	    3, 4, 5, 6,             // 2:1 (downsample)
	], vec![(0, 10000), (2, 20000)]),
	// slice 2
	MFWTick::new(vec![
	    7, 8, 9,                // 1:2 (upsample)
	], vec![(0, 5000)]),
	// slice 3
	MFWTick::new(vec![
	    10, 20, 30, 40, 50, 60  // 1.5:1 (downsample)
	], vec![(0, 15000)]),
	// slice 4
	MFWTick::new(vec![
	    10000, // fake
	], vec![(0, 10)]),
	]);
    let lf = LinearFilter::nw(20000, 10000, Rc::new(RefCell::new(flexwriter)));
    let c1 = sbar.sync(Rc::new(RefCell::new(lf)));

    if pcmsync::SYNC_STRATEGY_MAX {
    // -------------------- SYNC_STRATEGY_MAX
	const LEN : usize = 17;
	cread(c0.clone(), &mut data0[..LEN]);
	assert_eq!([10.0, 11.0,
		    -11.0, -11.0, // repeat to fill
		    // sync on ts1
		    80.0, 81.0, 82.0, 83.0, 84.0, 85.0, 86.0, 07.0, 88.0, 09.0,
		    // sync on ts2
		    14.0, 15.0, 16.0
        ],
		   data0[..LEN]);

	cread(c1.clone(), &mut data1[..LEN]);
	assert_eq!([1.0, 2.0,
		    3.0, 5.0,
		    // sync on ts1
		    7.0, 7.5, 8.0, 8.5, 9.0, 9.0,
		    -2.0, -2.0, -2.0, -2.0, // repeat to fill
		    // sync on ts2
		    10.0, 25.0, 40.0,
	],
		   data1[..LEN]);
    } else {
	// -------------------- SYNC_STRATEGY_AVT
	cread(c0.clone(), &mut data0[..14]);
	assert_eq!([10.0, 11.0,
		    -11.0, // repeat to fill
		    // sync on ts1
		    80.0, 81.0, 82.0, 83.0, 84.0, 85.0, 86.0, 07.0, // 88.0, 09.0, // discard during sync
		    // sync on ts2
		    14.0, 15.0, 16.0
        ],
		   data0[..14]);

	cread(c1.clone(), &mut data1[..14]);
	assert_eq!([1.0, 2.0,
		    3.0, // 5.0, // discarded during sync
		    // sync on ts1
		    7.0, 7.5, 8.0, 8.5, 9.0, 9.0,
		    -2.0, -2.0, // repeat to fill
		    // sync on ts2
		    10.0, 25.0, 40.0,
	],
		   data1[..14]);
    }
}
