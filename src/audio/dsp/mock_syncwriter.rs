// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::{cell::RefCell, rc::Rc};
#[cfg(test)]
use crate::audio::Freq;

#[cfg(test)]
use super::writer::{Timeslice, RcSyncWriter, PCMSyncWriter, FrequencyTrait, SyncPCMResult};

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
pub fn mock_rsw(name : String, ops : Vec<T>) -> RcSyncWriter {
    return Rc::new(RefCell::new(MockASW {
	name,
	ops : VecDeque::from(ops),
	repeat_me_if_stuck : -1.11111,
	stuck : None,
    }));
}
