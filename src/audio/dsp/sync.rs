#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::{rc::Rc, sync::{Arc, Mutex}, ops::DerefMut, cell::RefCell};

use crate::audio::Freq;

use super::{writer::{Timeslice, ArcSyncWriter, FrequencyTrait, PCMWriter, SyncPCMResult, PCMSyncBarrier, ArcWriter}, ringbuf::RingBuf};

// Implementations of audio stream synchronisation tools

const MAX_BUFFER_SIZE : usize = 4096;

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
	    todo!("fixme: this might fail and need two writes");
	    samples_offered_by_our_buffer = wrbuf.len();
	    wr.write_sync_pcm(wrbuf)
	};
	match result {
	    SyncPCMResult::Flush                           => {
		self.next_timeslice = None;
		self.buf.reset();
		return false;
	    },
	    SyncPCMResult::Wrote(written, None)            => {
		self.written += written;
		if self.written != samples_offered_by_our_buffer {
		    panic!("Unexpectedly received fewer bytes than requested");
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
	    return self.read_pcm(expected - self.buf.len());
	}
	return true;
    }

    fn fill_timeslice(&mut self) -> bool {
	if let None = self.next_timeslice {
	    return self.read_pcm(self.buf.remaining_capacity());
	}
	return true;
    }

    fn max_read(&self) -> usize {
	return self.buf.remaining_capacity();
    }

    /// Write as much as possible; return # of bytes written
    fn write_pcm(&mut self, outbuf : &mut [f32]) -> usize {
	let to_write = usize::min(self.max_read(), outbuf.len());
	self.buf.write_to(outbuf);
	return to_write;
    }
}

struct BasicWriterSyncImpl {
    sources : Vec<BasicWriterState>,
}

impl BasicWriterSyncImpl {
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
	    writer : writer.clone(),
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
	if oks == 0 {
	    debug!("All sources flushed");
	} else if oks == num_sources {
	    debug!("All sources reported success");
	    let timeslice = self.sources[0].next_timeslice;
	    let mut sum_offset = 0;

	    for (index, state) in self.sources.iter_mut().enumerate() {
		sum_offset += state.buf_pos_at_which_timeslice_could_start;
		if timeslice != state.next_timeslice {
		    warn!("Source #{index} disagrees about timeslice: {:?} vs. {timeslice:?}", state.next_timeslice);
		}
	    }

	    let avg_offset = sum_offset / num_sources;
	    debug!("  Setting slice length to {avg_offset}");

	    for state in self.sources.iter_mut() {
		if !state.fill_until(avg_offset) {
		    panic!("Unexpected granular flush");
		}
		if let Some(timeslice) = timeslice {
		    state.advance(avg_offset, timeslice);
		}
	    }
	    debug!("  Completed timeslice {timeslice:?}");

	} else {
	    panic!("Inconsistent flush: {}/{} sources flushed", num_sources - oks, num_sources);
	}
    }

    /// Handle a write request for the specified writer
    fn write_for(&mut self, writer_nr : usize, output : &mut [f32]) {
	let source = &mut self.sources[writer_nr];
	let write_pos = 0;
	let num_written = source.write_pcm(&mut output[write_pos..]);

	if num_written < output.len() {
	    // Ran out of buffer?
	    source.reset_buf_readwrite_pos();
	    self.prefill_buffers();
	}
    }

    // fn add_written(&mut self, writer_nr: usize, size : usize) {
    // 	self.sources[writer_nr].written += size;
    // }

    // fn flush(&mut self, writer_nr: usize) {
    // 	self.sources[writer_nr].next_timeslice = None;
    // 	self.sources[writer_nr].written = 0;
    // }

    // fn report_timeslice(&mut self, writer_nr: usize, timeslice : Timeslice) {
    // 	let boundary_pos = self.sources[writer_nr].written;
    // 	self.sources[writer_nr].next_timeslice = Some(timeslice);
    // 	self.sources[writer_nr].buf_pos_at_which_timeslice_could_start = boundary_pos;
    // }

    // fn requesting(&mut self, samples : usize) -> usize {
    // 	// check all writers for their state
    // 	// 
    // }
}


// ----------------------------------------

struct WriterSyncFwd {
    wsync : Rc<RefCell<BasicWriterSyncImpl>>,
    writer : ArcSyncWriter,
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
	self.wsync.borrow_mut().write_for(self.writer_nr, output);
    }
    // fn write_pcm(&mut self, output : &mut [f32]) {
    // 	let mut write_pos = 0;
    // 	loop {
    // 	    let result = {
    // 		let mut guard = self.writer.lock().unwrap();
    // 		let wr = guard.deref_mut();
    // 		wr.write_sync_pcm(&mut output[write_pos..])
    // 	    };
    // 	    let mut wsync = self.wsync.borrow_mut();
    // 	    match result {
    // 		SyncPCMResult::Flush                        => {
    // 		    wsync.deref_mut().flush(self.writer_nr);
    // 		    continue
    // 		},
    // 		SyncPCMResult::Wrote(size, None)            => {
    // 		    wsync.add_written(self.writer_nr, size);
    // 		    return;
    // 		},
    // 		SyncPCMResult::Wrote(size, Some(timeslice)) => {
    // 		    write_pos += size;
    // 		    wsync.add_written(self.writer_nr, size);
    // 		    wsync.report_timeslice(self.writer_nr, timeslice);
    // 		},
    // 	    }
    // 	}
    // }
}

// ----------------------------------------

struct PCMBasicSyncBarrier {
    sync : Rc<RefCell<BasicWriterSyncImpl>>,
}

impl PCMSyncBarrier for PCMBasicSyncBarrier {
    fn sync(&mut self, writer : ArcSyncWriter) -> ArcWriter {
        return Arc::new(Mutex::new(BasicWriterSyncImpl::synchronizer_for(self.sync.clone(), writer)))
    }
}
