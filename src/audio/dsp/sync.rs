#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::{rc::Rc, sync::{Arc, Mutex}, ops::DerefMut, cell::RefCell};

use crate::audio::Freq;

use super::{writer::{Timeslice, ArcSyncWriter, FrequencyTrait, PCMWriter, SyncPCMResult, PCMSyncBarrier, ArcWriter}, ringbuf::RingBuf};

#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use super::writer::PCMSyncWriter;

// ----------------------------------------
// Basic Sync barrier implementation
// Assumes that we never have more than MAX_BUFFER_SIZE output samples to produce per timeslices

const MAX_BUFFER_SIZE : usize = 4096;

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
    fn sync(&mut self, writer : ArcSyncWriter) -> ArcWriter {
        return Arc::new(Mutex::new(BasicWriterSyncImpl::synchronizer_for(self.sync.clone(), writer)))
    }
}

// ----------------------------------------
// Implementation

struct BasicWriterState {
    next_timeslice : Option<Timeslice>,
    buf_pos_at_which_timeslice_could_start : usize,
    written : usize,

    source : ArcSyncWriter,

    buf : RingBuf,
}

impl BasicWriterState {
    /// Read into local buffer
    /// Returns FALSE if flushed
    fn read_pcm(&mut self, count : usize) -> bool {
	if count == 0 {
	    return true;
	}
	let samples_offered_by_our_buffer;
	let result = {
	    let mut guard = self.source.lock().unwrap();
	    let wr = guard.deref_mut();
	    let wrbuf = self.buf.wrbuf(count);
	    samples_offered_by_our_buffer = wrbuf.len();
	    let result = wr.write_sync_pcm(wrbuf);
	    if let SyncPCMResult::Wrote(actual_count, None) = result {
		if samples_offered_by_our_buffer < count {
		    let wrbuf2 = self.buf.wrbuf(count - actual_count);
		    wr.write_sync_pcm(wrbuf2)
		} else { result }
	    } else { result} };
	match result {
	    SyncPCMResult::Flush                           => {
		self.next_timeslice = None;
		self.buf.reset();
		return false;
	    },
	    SyncPCMResult::Wrote(written, None)            => {
		self.written += written;
		if written != samples_offered_by_our_buffer {
		    panic!("Unexpectedly received fewer bytes than requested {}/{}", written, samples_offered_by_our_buffer);
		}
		return true;
	    },
	    SyncPCMResult::Wrote(written, Some(timeslice)) => {
		self.buf.unread(samples_offered_by_our_buffer - written).unwrap();
		self.written += written;
		self.buf_pos_at_which_timeslice_could_start = self.written;
		self.next_timeslice = Some(timeslice);
		return true;
	    },
	}
    }

    fn advance(&mut self, write_pos : usize, timeslice : Timeslice) {
	let mut guard = self.source.lock().unwrap();
	let wr = guard.deref_mut();
	wr.advance_sync(timeslice);
	self.next_timeslice = None;
	self.buf.unread(self.written - write_pos).unwrap();
	self.written = 0;
	self.buf_pos_at_which_timeslice_could_start = 0;
    }

    fn fill_until(&mut self, expected : usize) -> bool {
	if expected > self.written {
	    return self.read_pcm(expected - self.written);
	}
	return true;
    }

    fn fill_timeslice(&mut self) -> bool {
	if let None = self.next_timeslice {
	    return self.read_pcm(self.buf.remaining_capacity());
	}
	return true;
    }

    /// Write as much as possible; return # of bytes written
    fn write_pcm(&mut self, outbuf : &mut [f32]) -> usize {
	return self.buf.write_to(outbuf);
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

    fn synchronizer_for(rself : Rc<RefCell<Self>>, writer: ArcSyncWriter) -> WriterSyncFwd {
	let freq = {
	    let guard = writer.lock().unwrap();
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
	for state in self.sources.iter_mut() {
	    if state.fill_timeslice() {
		oks += 1;
	    }
	}
	println!("[BWSI]   prefill check -> {oks}");
	if oks == 0 {
	    println!("All sources flushed");
	} else if oks == num_sources {
	    println!("All sources reported success");
	    let timeslice = self.sources[0].next_timeslice;
	    let mut sum_offset = 0;

	    for (index, state) in self.sources.iter_mut().enumerate() {
		sum_offset += state.buf_pos_at_which_timeslice_could_start;
		if timeslice != state.next_timeslice {
		    warn!("Source #{index} disagrees about timeslice: {:?} vs. {timeslice:?}", state.next_timeslice);
		}
	    }

	    let avg_offset = sum_offset / num_sources;
	    println!("  Setting slice length to {avg_offset}");

	    for state in self.sources.iter_mut() {
		if !state.fill_until(avg_offset) {
		    panic!("Unexpected granular flush");
		}
		if let Some(timeslice) = timeslice {
		    state.advance(avg_offset, timeslice);
		}
	    }
	    println!("  Completed timeslice {timeslice:?}");

	} else {
	    panic!("Inconsistent flush: {}/{} sources flushed", num_sources - oks, num_sources);
	}
    }

    /// Handle a write request for the specified writer
    fn write_for(&mut self, writer_nr : usize, output : &mut [f32]) {
	let mut write_pos = 0;
	let mut last_write_pos = output.len() + 1; // something different to avoid triggering the sanity check
	println!("[BWSI] writer {writer_nr} wants {} samples", output.len());
	while write_pos < output.len() {
	    let source = &mut self.sources[writer_nr];
	    let num_written = source.write_pcm(&mut output[write_pos..]);
	    println!("[BWSI]  wrote {num_written}");

	    if num_written == 0 && last_write_pos == write_pos {
		panic!("No progress: {}/{}/{}; buf={}/{}.  is the source really producing ticks?  Is our buffer big enough?",
		       last_write_pos, write_pos, output.len(), source.buf.len(), source.buf.capacity());
	    }

	    last_write_pos = write_pos;
	    write_pos += num_written;

	    if write_pos < output.len() {
		// Ran out of buffer?
		//source.reset_buf_readwrite_pos();
		println!("[BWSI]   ran out of buffer, must prefill");
		self.prefill_buffers();
	    }
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
	println!("[WSF:{}] Forwarding write request of size {}", self.writer_nr, output.len());
	self.wsync.borrow_mut().write_for(self.writer_nr, output);
    }
}

// ========================================
// Testing

// ----------------------------------------
// Helpers

#[cfg(test)]
enum T {
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

// ----------------------------------------
// Actual tests

#[cfg(test)]
fn mock_asw(name : String, ops : Vec<T>) -> ArcSyncWriter {
    return Arc::new(Mutex::new(MockASW {
	name,
	ops : VecDeque::from(ops),
	repeat_me_if_stuck : -1.11111,
	stuck : None,
    }));
}

#[cfg(test)]
fn cread(writer : ArcWriter, dest : &mut [f32]) {
    let mut guard = writer.lock().unwrap();
    let wr = guard.deref_mut();
    wr.write_pcm(dest);
}

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
    let mut data0 = [0.0; 8];
    let mut data1 = [0.0; 8];
    let mut sbar = PCMBasicSyncBarrier::new();
    let c0 = sbar.sync(mock_asw("0".to_string(), vec![
	T::S(vec![10.0, 11.0]),
	T::TS(-11.0, 1),
	T::S(vec![12.0, 13.0]),
	T::TS(-12.0, 2),
	T::S(vec![14.0, 15.0, 16.0, 17.0]),
	T::TS(-13.0, 2),
    ]));
    let c1 = sbar.sync(mock_asw("1".to_string(), vec![
	T::S(vec![20.0, 21.0, 22.0, 23.0]),
	T::TS(-21.0, 1),
	T::S(vec![24.0, 25.0]),
	T::TS(-22.0, 2),
	T::S(vec![26.0, 27.0]),
	T::TS(-23.0, 2),
    ]));
    cread(c0.clone(), &mut data0[0..8]);
    assert_eq!([10.0, 11.0, -11.0, 12.0, 13.0, 14.0, 15.0, 16.0],
	       data0[..]);

    cread(c1.clone(), &mut data1[0..8]);
    assert_eq!([20.0, 21.0, 22.0, 24.0, 25.0, 26.0, 27.0, -23.0],
	       data1[..]);
}

