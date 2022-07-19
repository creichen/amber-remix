// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use crate::util::IndexLen;
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{writer::{PCMSyncWriter, Timeslice, SyncPCMResult, FrequencyTrait, RcSyncWriter}, ringbuf::RingBuf};
use std::{ops::DerefMut, rc::Rc, cell::RefCell};


pub struct LinearCrossfade {
    writer : RcSyncWriter,
    buf : RingBuf,
}

impl LinearCrossfade {
    pub fn new(fade_window_size : usize, writer : RcSyncWriter) -> LinearCrossfade {
	LinearCrossfade {
	    writer,
	    buf : RingBuf::new(fade_window_size),
	}
    }

    pub fn new_rc(fade_window_size : usize, writer : RcSyncWriter) -> RcSyncWriter {
	return Rc::new(RefCell::new(LinearCrossfade::new(fade_window_size, writer)));
    }
}

impl FrequencyTrait for LinearCrossfade {
    fn frequency(&self) -> crate::audio::Freq {
	return self.writer.borrow().frequency();
    }
}

impl PCMSyncWriter for LinearCrossfade {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
	let result = self.writer.borrow_mut().write_sync_pcm(output);

	if self.buf.len() > 0 {
	    // We are currently cross-fading?
	    let fade_size = usize::min(self.buf.len(), output.len());

	    let factor = (self.buf.capacity() + 1) as f32;

	    // Previous timeslice buffer
	    let prev_buf = self.buf.peek_front(fade_size);
	    let prev_offset = self.buf.remaining_capacity(); // This many we already wrote

	    for i in 0..fade_size {
		let new_val = output[i];
		let old_val = prev_buf.get(i);
		let new_val_contribution = (1 + prev_offset + i) as f32;
		let old_val_contribution = factor - new_val_contribution;
		output[i] = ((new_val * new_val_contribution) + (old_val * old_val_contribution)) / factor;
	    }
	    self.buf.drop_front(fade_size).unwrap();
	}
	return result;
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
	while !self.buf.is_full() {
	    let wrbuf = self.buf.wrbuf(self.buf.remaining_capacity());
	    match self.writer.borrow_mut().write_sync_pcm(wrbuf) {
		SyncPCMResult::Wrote(amount, _) => { assert_eq!(amount, wrbuf.len()); },
		SyncPCMResult::Flush            => { self.buf.reset();
						     break; },
	    }
	}
	self.writer.borrow_mut().deref_mut().advance_sync(timeslice);
    }
}

// ========================================
// Testing

#[cfg(test)]
use std::collections::VecDeque;

// ----------------------------------------
// Test helpers
#[cfg(test)]
struct DWSlice {
    before: VecDeque<f32>,
    after: VecDeque<f32>,
}

#[cfg(test)]
struct DW {
    slices : Vec<DWSlice>,
    slice_index : usize,
}

#[cfg(test)]
impl DW {
    pub fn new(slices : Vec<(Vec<f32>, Vec<f32>)>) -> Rc<RefCell<DW>> {
	Rc::new(RefCell::new(DW {
	    slices : slices.iter().map(|(before, after)| DWSlice { before : VecDeque::from(before[..].to_vec()), after : VecDeque::from(after[..].to_vec()) }).collect(),
	    slice_index : 0,
	}))
    }
}

#[cfg(test)]
impl FrequencyTrait for DW {
    fn frequency(&self) -> crate::audio::Freq {
	return 1000;
    }
}

#[cfg(test)]
impl PCMSyncWriter for DW {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
	let slice = &mut self.slices[self.slice_index];
	let bytes_written = if !slice.before.is_empty() {
	    // before
	    let to_write = usize::min(slice.before.len(), output.len());
	    for i in 0..to_write {
		output[i] = slice.before.pop_front().unwrap();
	    };
	    to_write
	} else {
	    // after
	    let to_write = usize::min(slice.after.len(), output.len());
	    for i in 0..to_write {
		output[i] = slice.after.pop_front().unwrap();
	    };
	    to_write
	};
	return SyncPCMResult::Wrote(bytes_written,
				    if slice.before.is_empty() {
					Some(self.slice_index)
				    } else {
					None
				    });
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
        assert_eq!(timeslice, self.slice_index);
	assert_eq!(0, self.slices[self.slice_index].before.len());
	assert_eq!(0, self.slices[self.slice_index].after.len());
	self.slice_index += 1;
    }
}

// ----------------------------------------
// Tests

#[cfg(test)]
#[test]
pub fn test_simple_1() {
    let mut outbuf = [0.0; 6];
    let testdw = DW::new(vec![
	(vec![1.0, 2.0, 3.0], vec![10.0]),
	(vec![8.0, 8.0, 8.0], vec![]),
    ]);
    let mut lcf = LinearCrossfade::new(1, testdw);
    assert_eq!(SyncPCMResult::Wrote(3, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf));
    lcf.advance_sync(0);
    assert_eq!(SyncPCMResult::Wrote(3, Some(1)),
	       lcf.write_sync_pcm(&mut outbuf[3..]));
    assert_eq!([1.0, 2.0, 3.0, 9.0, 8.0, 8.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
pub fn test_simple_4() {
    let mut outbuf = [0.0; 8];
    let testdw = DW::new(vec![
	(vec![1.0, 2.0, 3.0], vec![10.0, 11.0, 12.0, 13.0]),
	(vec![8.0, 8.0, 8.0, 8.0, 8.0, 8.0], vec![]),
    ]);
    let mut lcf = LinearCrossfade::new(4, testdw);
    assert_eq!(SyncPCMResult::Wrote(3, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf));
    lcf.advance_sync(0);
    assert_eq!(SyncPCMResult::Wrote(5, None),
	       lcf.write_sync_pcm(&mut outbuf[3..]));
    assert_eq!([1.0, 2.0, 3.0, 9.6, 9.8, 9.6, 9.0, 8.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
pub fn test_4_interrupted() {
    for n in 1..5 {
	let mut outbuf = [0.0; 8];
	let testdw = DW::new(vec![
	    (vec![1.0, 2.0, 3.0], vec![10.0, 11.0, 12.0, 13.0]),
	    (vec![8.0, 8.0, 8.0, 8.0, 8.0, 8.0], vec![]),
	]);
	let mut lcf = LinearCrossfade::new(4, testdw);
	assert_eq!(SyncPCMResult::Wrote(3, Some(0)),
		   lcf.write_sync_pcm(&mut outbuf));
	lcf.advance_sync(0);

	assert_eq!(SyncPCMResult::Wrote(n, None),
		   lcf.write_sync_pcm(&mut outbuf[3..3+n]));
	assert_eq!(SyncPCMResult::Wrote(5-n, None),
		   lcf.write_sync_pcm(&mut outbuf[3+n..]));

	assert_eq!([1.0, 2.0, 3.0, 9.6, 9.8, 9.6, 9.0, 8.0],
		   &outbuf[..]);
    }
}

#[cfg(test)]
#[test]
pub fn test_twice_3() {
    let mut outbuf = [0.0; 11];
    let testdw = DW::new(vec![
	(vec![1.0, 2.0, 3.0], vec![10.0, 10.0, 10.0]),
	(vec![14.0, 14.0, 14.0, 14.0], vec![20.0, 20.0, 20.0]),
	(vec![24.0, 24.0, 24.0, 24.0], vec![20.0, 20.0]),
    ]);
    let mut lcf = LinearCrossfade::new(3, testdw);
    assert_eq!(SyncPCMResult::Wrote(3, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf));
    lcf.advance_sync(0);
    assert_eq!(SyncPCMResult::Wrote(4, Some(1)),
	       lcf.write_sync_pcm(&mut outbuf[3..]));
    lcf.advance_sync(1);
    assert_eq!(SyncPCMResult::Wrote(4, Some(2)),
	       lcf.write_sync_pcm(&mut outbuf[7..]));
    assert_eq!([1.0, 2.0, 3.0,
		11.0, 12.0, 13.0, 14.0,
		21.0, 22.0, 23.0, 24.0],
	       &outbuf[..]);
}

#[cfg(test)]
#[test]
pub fn test_fillup_passthrough() {
    let mut outbuf = [0.0; 7];
    let testdw = DW::new(vec![
	(vec![1.0, 2.0, 3.0], vec![10.0, 20.0, 20.0, 20.0]),
	(vec![30.0, 30.0, 30.0], vec![]),
    ]);
    let mut lcf = LinearCrossfade::new(3, testdw);
    assert_eq!(SyncPCMResult::Wrote(3, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf));
    assert_eq!(SyncPCMResult::Wrote(1, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf[3..4]));
    lcf.advance_sync(0);

    assert_eq!(SyncPCMResult::Wrote(3, Some(1)),
	       lcf.write_sync_pcm(&mut outbuf[4..]));
    assert_eq!([1.0, 2.0, 3.0, 10.0, 22.5, 25.0, 27.5],
	       &outbuf[..]);
}

