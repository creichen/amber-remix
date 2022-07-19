// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{writer::{PCMSyncWriter, Timeslice, SyncPCMResult, FrequencyTrait}, ringbuf::RingBuf};
use std::{sync::{Mutex, Arc}, collections::VecDeque};


pub struct LinearCrossfade {
    writer : Arc<Mutex<dyn PCMSyncWriter>>,
    buf : RingBuf,
    transition_mode : bool,
}

impl LinearCrossfade {
    pub fn new(fade_window_size : usize, writer : Arc<Mutex<dyn PCMSyncWriter>>) -> LinearCrossfade {
	LinearCrossfade {
	    writer,
	    buf : RingBuf::new(fade_window_size),
	    transition_mode : false,
	}
    }
}

impl FrequencyTrait for LinearCrossfade {
    fn frequency(&self) -> crate::audio::Freq {
	let guard = self.writer.lock().unwrap();
	return guard.frequency();
    }
}

impl PCMSyncWriter for LinearCrossfade {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
        todo!()
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
        todo!()
    }
}

// ========================================
// Testing

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
    pub fn new(slices : Vec<(Vec<f32>, Vec<f32>)>) -> Arc<Mutex<DW>> {
	Arc::new(Mutex::new(DW {
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
        todo!()
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
        todo!()
    }
}

// ----------------------------------------
// Tests

#[ignore]
#[cfg(test)]
#[test]
pub fn test_simple_1() {
    let mut outbuf = [0.0; 6];
    let mut testdw = DW::new(vec![
	(vec![1.0, 2.0, 3.0], vec![10.0]),
	(vec![8.0, 8.0, 8.0], vec![]),
    ]);
    let mut lcf = LinearCrossfade::new(1, testdw);
    assert_eq!(SyncPCMResult::Wrote(4, Some(0)),
	       lcf.write_sync_pcm(&mut outbuf));
    lcf.advance_sync(0);
    assert_eq!(SyncPCMResult::Wrote(2, None),
	       lcf.write_sync_pcm(&mut outbuf[4..]));
    assert_eq!([1.0, 2.0, 3.0, 9.0, 8.0, 8.0],
	       &outbuf[..]);
}


#[ignore]
#[cfg(test)]
#[test]
pub fn test_simple_4() {
    // let mut outbuf = [0.0; 6];
    // let mut testdw = DW::new(vec![
    // 	(vec![1.0, 2.0, 3.0], vec![10.0]),
    // 	(vec![8.0, 8.0, 8.0], vec![]),
    // ]);
    // let mut lcf = LinearCrossfade::new(1, testdw);
    // assert_eq!(SyncPCMResult::Wrote(4, Some(0)),
    // 	       lcf.write_sync_pcm(&mut outbuf));
    // lcf.advance_sync(0);
    // assert_eq!(SyncPCMResult::Wrote(2, None),
    // 	       lcf.write_sync_pcm(&mut outbuf[4..]));
    // assert_eq!([1.0, 2.0, 3.0, 9.0, 8.0, 8.0],
    // 	       &outbuf[..]);
}

#[ignore]
#[cfg(test)]
#[test]
pub fn test_4_interrupted() {
    // let mut outbuf = [0.0; 6];
    // let mut testdw = DW::new(vec![
    // 	(vec![1.0, 2.0, 3.0], vec![10.0]),
    // 	(vec![8.0, 8.0, 8.0], vec![]),
    // ]);
    // let mut lcf = LinearCrossfade::new(1, testdw);
    // assert_eq!(SyncPCMResult::Wrote(4, Some(0)),
    // 	       lcf.write_sync_pcm(&mut outbuf));
    // lcf.advance_sync(0);
    // assert_eq!(SyncPCMResult::Wrote(2, None),
    // 	       lcf.write_sync_pcm(&mut outbuf[4..]));
    // assert_eq!([1.0, 2.0, 3.0, 9.0, 8.0, 8.0],
    // 	       &outbuf[..]);
}

