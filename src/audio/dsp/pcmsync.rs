// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::{rc::Rc, cell::RefCell};

use crate::audio::Freq;

use super::{writer::{Timeslice, RcSyncWriter, RcSyncBarrier, FrequencyTrait, PCMWriter, SyncPCMResult, PCMSyncBarrier, RcPCMWriter}, ringbuf::RingBuf};

// ----------------------------------------
// Main API

pub fn new_basic() -> RcSyncBarrier {
    return Rc::new(RefCell::new(PCMBasicSyncBarrier::new()));
}

// ----------------------------------------
// Basic Sync barrier implementation
// Assumes that we never have more than MAX_BUFFER_SIZE output samples to produce per timeslices

const MAX_BUFFER_SIZE : usize = 32768;

pub struct PCMBasicSyncBarrier {
    sync : Rc<RefCell<BasicWriterSyncImpl>>,
}

impl PCMBasicSyncBarrier {
    pub fn new() -> PCMBasicSyncBarrier {
	return PCMBasicSyncBarrier {
	    sync : Rc::new(RefCell::new(BasicWriterSyncImpl::new())),
	}
    }
}

impl PCMSyncBarrier for PCMBasicSyncBarrier {
    fn sync(&mut self, writer : RcSyncWriter) -> RcPCMWriter {
        return Rc::new(RefCell::new(BasicWriterSyncImpl::synchronizer_for(self.sync.clone(), writer)))
    }
}

// ----------------------------------------
// Implementation

struct BasicWriterState {
    next_timeslice : Option<Timeslice>,
    buf_pos_at_which_timeslice_could_start : usize,
    written : usize,

    source : RcSyncWriter,

    buf : RingBuf,
}


// If set, synchronise on the _longest_ tick, rather than the average
pub const SYNC_STRATEGY_MAX : bool = true;

// Hold back at least this many samples for synchronisation so that we can undo them if needed
const RESERVE : usize = 0;

impl BasicWriterState {
    /// Read into local buffer
    /// Returns FALSE if flushed
    fn read_pcm(&mut self, index : usize, requested_count : usize) -> bool {
	let count = requested_count;
	if count == 0 {
	    return true;
	}
	let mut written1 = 0;
	let mut samples_offered_by_our_buffer;
	let result = {
	    // let mut guard = self.source.lock().unwrap();
	    // let wr = guard.deref_mut();
	    let mut wr = self.source.borrow_mut();
	    let wrbuf = self.buf.wrbuf(count);
	    samples_offered_by_our_buffer = wrbuf.len();
	    let result = wr.write_sync_pcm(wrbuf);
	    if let SyncPCMResult::Wrote(actual_count, None) = result {
		if samples_offered_by_our_buffer < count {
		    written1 = actual_count;
		    let wrbuf2 = self.buf.wrbuf(count - actual_count);
		    samples_offered_by_our_buffer += wrbuf2.len();
		    wr.write_sync_pcm(wrbuf2)
		} else { result }
	    } else { result} };
	match result {
	    SyncPCMResult::Flush                           => {
		self.next_timeslice = None;
		self.buf.reset();
		debug!("---- This was source #{index}, reporting _Flush_");
		return false;
	    },
	    SyncPCMResult::Wrote(written, None)            => {
		let written = written + written1;
		self.written += written;
		if written != samples_offered_by_our_buffer {
		    panic!("Unexpectedly received fewer bytes than requested {}/{}", written, samples_offered_by_our_buffer);
		}
		self.written += written;
		debug!("---- This was source #{index}, reporting {} writes but no timeslice", self.written);
		return true;
	    },
	    SyncPCMResult::Wrote(written, Some(timeslice)) => {
		let written = written + written1;
		if written > samples_offered_by_our_buffer {
		    panic!("Somehow wrote more than possible: {written}/{samples_offered_by_our_buffer}; now {}", self.buf.len());
		}
		self.buf.drop_back(samples_offered_by_our_buffer - written).unwrap();
		self.written += written;
		self.buf_pos_at_which_timeslice_could_start = self.written;
		self.next_timeslice = Some(timeslice);
		debug!("---- This was source #{index}, reporting {} writes and a timeslice", self.written);
		return true;
	    },
	}
    }

    /// Advance to next time slice and drop excess buffer data, if any
    fn advance(&mut self, index : usize, write_pos : usize, timeslice : Timeslice) {
	let mut wr = self.source.borrow_mut();
	wr.advance_sync(timeslice);
	self.next_timeslice = None;
	if self.written < write_pos {
	    error!("While advancing source #{index} to timeslice {timeslice}: unexpectedly fewer written bytes than desired-- actual:{} expected:{write_pos}", self.written);
	} else {
	    info!("BEFORE-dropattempt({write_pos})(#{index}, avg_offset) {:p} {} (written={})", &self.buf, self.buf.internal(), self.written);
	    self.buf.drop_back(self.written - write_pos).unwrap();
	}
	self.written = 0;
	self.buf_pos_at_which_timeslice_could_start = 0;
    }

    /// Read into the local buffer until we have the desired level
    fn fill_until(&mut self, index : usize, expected : usize) -> bool {
	if expected > self.written {
	    return self.read_pcm(index, expected - self.written);
	}
	return true;
    }

    /// Read into the local buffer until we have the next timeslice
    fn fill_timeslice(&mut self, index : usize) -> bool {
	if let None = self.next_timeslice {
	    let result = self.read_pcm(index, self.buf.remaining_capacity());
	    debug!("source[#{index}].fill_timeslice(): now timeslice={:?}", self.next_timeslice);
	    debug!("  written = {}", self.written);
	    return result;
	}
	debug!("source[#{index}].fill_timeslice(): already at timeslice {:?}", self.next_timeslice);
	return true;
    }

    /// Write as much as possible (minus reserve); return # of bytes written
    fn write_pcm(&mut self, outbuf : &mut [f32]) -> usize {
	let available_count = if self.buf.len() < RESERVE { 0 } else { self.buf.len() - RESERVE };
	let count = usize::min(available_count, outbuf.len());
	return self.buf.write_to(&mut outbuf[..count]);
    }
}

struct BasicWriterSyncImpl {
    sources : Vec<BasicWriterState>,
}

impl BasicWriterSyncImpl {
    fn new() -> BasicWriterSyncImpl {
	return BasicWriterSyncImpl {
	    sources : Vec::new(),
	}
    }

    fn synchronizer_for(rself : Rc<RefCell<Self>>, writer: RcSyncWriter) -> WriterSyncFwd {
	let freq = {
	    // let guard = writer.lock().unwrap();
	    let guard = writer.borrow();
	    guard.frequency()
	};
	let writer_nr =	{
	    let mut mself = rself.borrow_mut();
	    let nr = mself.sources.len();
	    mself.sources.push(BasicWriterState {
		next_timeslice : None,
		buf_pos_at_which_timeslice_could_start : 0,
		written : 0,
		buf : RingBuf::new(MAX_BUFFER_SIZE),

		source : writer.clone(),
	    });
	    nr
	};
	return WriterSyncFwd {
	    wsync : rself.clone(),
	    writer_nr,
	    freq,
	};
    }

    /// Fill all writer states' buffers for the next tick, or otherwise as much as possible
    /// This handles synchronisation.
    fn prefill_buffers(&mut self) {
	let mut oks = 0;
	let num_sources = self.sources.len();
	for (index, state) in self.sources.iter_mut().enumerate() {
	    if state.fill_timeslice(index) {
		oks += 1;
	    }
	}
	trace!("[BWSI]   prefill check -> {oks}");
	if oks == 0 {
	    trace!("All sources flushed");
	} else if oks == num_sources {
	    trace!("All sources reported success");
	    let timeslice = self.sources[0].next_timeslice;
	    if None==timeslice {
		error!("Buffers have not reached timeslice yet:");
		for (index, state) in self.sources.iter_mut().enumerate() {
		    error!("  #!{index}: {:?} size={}/{}", state.next_timeslice, state.buf.len(), state.buf.capacity());
		}
	    }
	    let mut sum_offset = 0;
	    let mut max_offset = 0;
	    let mut disagreement = false;

	    for (index, state) in self.sources.iter_mut().enumerate() {
		let offset = state.buf_pos_at_which_timeslice_could_start;
		sum_offset += offset;
		max_offset = usize::max(max_offset, offset);
		if timeslice != state.next_timeslice {
		    warn!("Source #{index} disagrees about timeslice: {:?} vs. {timeslice:?}", state.next_timeslice);
		    disagreement = true;
		}
	    }
	    if disagreement {
		self.print_status();
	    }

	    let sync_offset = if SYNC_STRATEGY_MAX { max_offset } else {
		// arithmetic mean offset
		sum_offset / num_sources
	    };
	    trace!("  Setting slice length to {sync_offset}");

	    for (index, state) in self.sources.iter_mut().enumerate() {
		info!("BEFORE-pcmfill(#{index}, avg_offset) {:p} {} (written={})", &state.buf, state.buf.internal(), state.written);
		if !state.fill_until(index, sync_offset) {
		    panic!("Unexpected granular flush");
		}
		info!("AFTER-pcmfill(#{index}, avg_offset) {:p} {} (written={})", &state.buf, state.buf.internal(), state.written);
		if let Some(timeslice) = timeslice {
		    state.advance(index, sync_offset, timeslice);
		}
	    }
	    debug!("  Completed timeslice {timeslice:?}");

	} else {
	    panic!("Inconsistent flush: {}/{} sources flushed", num_sources - oks, num_sources);
	};
    }

    fn print_status(&self) {
	info!("[PCMSYNC] Status:");
	for (index, state) in self.sources.iter().enumerate() {
	    info!("[PCMSYNC] #{index} : timeslice {:?} starting @{}",
		  state.next_timeslice, state.buf_pos_at_which_timeslice_could_start);
	    info!("[PCMSYNC]     written : {}", state.written);
	    info!("[PCMSYNC]     buf : {} / {}", state.buf.len(), state.buf.capacity());
	}
    }

    /// Handle a write request for the specified writer
    fn write_for(&mut self, writer_nr : usize, output : &mut [f32]) {
	let mut write_pos = 0;
	let mut last_write_pos = output.len() + 1; // something different to avoid triggering the sanity check
	trace!("[BWSI] writer {writer_nr} wants {} samples", output.len());
	while write_pos < output.len() {
	    let source = &mut self.sources[writer_nr];
	    let source_buflen_before_write = source.buf.len();
	    let source_bufcapacity = source.buf.capacity();
	    let num_written = source.write_pcm(&mut output[write_pos..]);
	    trace!("[BWSI]  wrote {num_written}");

	    let source_buflen_after_write = source.buf.len();
	    if write_pos < output.len() {
		// Ran out of buffer?
		//source.reset_buf_readwrite_pos();
		info!("[BWSI]   ran out of buffer, must prefill");
		self.prefill_buffers();
	    }
	    let source_buflen_after_prefill = self.sources[writer_nr].buf.len();

	    if num_written == 0 && last_write_pos == write_pos {
		self.print_status();
		panic!("No progress: lastwritepos:{}/writepos:{}/outlen:{}; buf_capacity={}, buflens=(start:{}/post-write:{}/post-prefill:{}).  Is the source really producing ticks?  Is our buffer big enough?",
		       last_write_pos, write_pos, output.len(), source_bufcapacity,
		       source_buflen_before_write, source_buflen_after_write, source_buflen_after_prefill);
	    }

	    last_write_pos = write_pos;
	    write_pos += num_written;
	}
    }
}


// ----------------------------------------

struct WriterSyncFwd {
    wsync : Rc<RefCell<BasicWriterSyncImpl>>,
    writer_nr : usize,
    freq : Freq,
}

impl WriterSyncFwd {
}

impl FrequencyTrait for WriterSyncFwd {
    fn frequency(&self) -> Freq {
	return self.freq;
    }
}

impl PCMWriter for WriterSyncFwd {
    fn write_pcm(&mut self, output : &mut [f32]) {
	trace!("[WSF:{}] Forwarding write request of size {}", self.writer_nr, output.len());
	self.wsync.borrow_mut().write_for(self.writer_nr, output);
    }
}

// ========================================
// Testing

#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use super::writer::PCMSyncWriter;
#[cfg(test)]
use crate::audio::dsp::pcmsync;

// ----------------------------------------
// Helpers

#[cfg(test)]
pub enum T {
    S(Vec<f32>),
    TS(f32, Timeslice), // repeat the first f32 until the timeslice is advanced to
}

#[cfg(test)]
struct MockASW {
    name : String,
    ops : VecDeque<T>,
    repeat_me_if_stuck : f32,
    stuck : Option<Timeslice>,
}

#[cfg(test)]
impl FrequencyTrait for MockASW {
    fn frequency(&self) -> Freq {
	return 42;
    }
}

#[cfg(test)]
impl PCMSyncWriter for MockASW {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
	let mut write_pos = 0;
	let write_end = output.len();
	while write_pos < write_end {
	    if let Some(_) = self.stuck {
		println!("MockASW:{}] Stuck, writing {} x {}", self.name, self.repeat_me_if_stuck, write_end-write_pos);
		// Waiting for time slice
		output[write_pos..].fill(self.repeat_me_if_stuck);
		write_pos = write_end;
	    } else {
		match self.ops.pop_front() {
		    None => {
			let dummydata = vec![1000.01, 1001.01, 1002.01, 1003.01];
			self.ops.push_front(T::S(dummydata));
		    }
		    Some(T::S(opvec))        => {
			let len = usize::min(write_end - write_pos,
					     opvec.len());
			println!("MockASW:{}] Writing {:?}", self.name, &opvec[..len]);
			output[write_pos..write_pos+len].copy_from_slice(&opvec[..len]);
			write_pos += len;
			if len < opvec.len() {
			    self.ops.push_front(T::S(Vec::from(&opvec[len..])));
			}
		    }
		    Some(T::TS(fill, slice)) => {
			println!("MockASW:{}] Hit TS {:?} with {write_pos} written", self.name, slice);
			self.repeat_me_if_stuck = fill;
			self.stuck = Some(slice);
			return SyncPCMResult::Wrote(write_pos, Some(slice));
		    }
		}}
	};
	return SyncPCMResult::Wrote(write_pos, None);
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
	assert_eq!(Some(timeslice), self.stuck);
	self.stuck = None;
    }
}

#[cfg(test)]
pub fn mock_asw(name : String, ops : Vec<T>) -> RcSyncWriter {
    return Rc::new(RefCell::new(MockASW {
	name,
	ops : VecDeque::from(ops),
	repeat_me_if_stuck : -1.11111,
	stuck : None,
    }));
}

#[cfg(test)]
pub fn cread(writer : RcPCMWriter, dest : &mut [f32]) {
//    let mut guard = writer.lock().unwrap();
//    let wr = guard.deref_mut();
    let mut wr = writer.borrow_mut();
    wr.write_pcm(dest);
}

// ----------------------------------------
// Tests

#[cfg(test)]
#[test]
fn test_unary_passthrough_boundary() {
    let mut data0 = [0.0; 6];
    let mut sbar = PCMBasicSyncBarrier::new();
    let c0 = sbar.sync(mock_asw("0".to_string(), vec![
	T::S(vec![1.0, 2.0, 3.0, 4.0, 5.0]),
	T::TS(-1.0, 1),
	T::S(vec![6.0, 7.0]),
	T::TS(-2.0, 2),
    ]));
    cread(c0.clone(), &mut data0[0..5]);
    cread(c0.clone(), &mut data0[5..6]);
    assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
	       data0[..]);
}

#[cfg(test)]
#[test]
fn test_unary_passthrough_cross_boundary() {
    let mut data0 = [0.0; 6];
    let mut sbar = PCMBasicSyncBarrier::new();
    let c0 = sbar.sync(mock_asw("0".to_string(), vec![
	T::S(vec![1.0, 2.0, 3.0, 4.0, 5.0]),
	T::TS(-1.0, 1),
	T::S(vec![6.0, 7.0]),
	T::TS(-2.0, 2),
    ]));
    cread(c0.clone(), &mut data0[0..2]);
    cread(c0.clone(), &mut data0[2..6]);
    assert_eq!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
	       data0[..]);
}

#[cfg(test)]
#[test]
fn test_binary() {
    let mut data0 = [0.0; 10];
    let mut data1 = [0.0; 10];
    let mut sbar = PCMBasicSyncBarrier::new();
    let c0 = sbar.sync(mock_asw("0".to_string(), vec![
	T::S(vec![10.0, 11.0]),
	T::TS(-11.0, 1),
	T::S(vec![12.0, 13.0]),
	T::TS(-12.0, 2),
	T::S(vec![14.0, 15.0, 16.0, 17.0]),
	T::TS(-13.0, 3),
    ]));
    let c1 = sbar.sync(mock_asw("1".to_string(), vec![
	T::S(vec![20.0, 21.0, 22.0, 23.0]),
	T::TS(-21.0, 1),
	T::S(vec![24.0, 25.0]),
	T::TS(-22.0, 2),
	T::S(vec![26.0, 27.0]),
	T::TS(-23.0, 3),
    ]));
    if pcmsync::SYNC_STRATEGY_MAX {
    // -------------------- SYNC_STRATEGY_MAX
	cread(c0.clone(), &mut data0[0..10]);
	assert_eq!([10.0, 11.0, -11.0, -11.0,
		    12.0, 13.0,
		    14.0, 15.0, 16.0, 17.0],
		   data0[..]);

	cread(c1.clone(), &mut data1[0..10]);
	assert_eq!([20.0, 21.0, 22.0, 23.0,
		    24.0, 25.0,
		    26.0, 27.0, -23.0, -23.0],
		   data1[..]);
    } else {
	// -------------------- SYNC_STRATEGY_AVT
	cread(c0.clone(), &mut data0[0..8]);
	assert_eq!([10.0, 11.0, -11.0, 12.0, 13.0, 14.0, 15.0, 16.0],
		   data0[..8]);

	cread(c1.clone(), &mut data1[0..8]);
	assert_eq!([20.0, 21.0, 22.0, 24.0, 25.0, 26.0, 27.0, -23.0],
		   data1[..8]);
    }
}

