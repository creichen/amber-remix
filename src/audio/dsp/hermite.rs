// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

// Hermite downsampling
// For downsampling from integer multiples of higher frequencies

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use crate::util::IndexLen;
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{writer::{PCMSyncWriter, Timeslice, SyncPCMResult, FrequencyTrait, RcSyncWriter}, ringbuf::RingBuf};
use std::{ops::DerefMut, rc::Rc, cell::RefCell};

struct Buf {
    buf  : Vec<f32>,
    zero : usize,
}

impl Buf {
    fn new(size : usize) -> Buf {
	assert_eq!(0, (size - 1) & size); // size must be a power of two
	Buf {
	    buf : vec![0.0; size],
	    zero : 0,
	}
    }

    // get from positive offset
    fn get(&self) -> f32 {
	return self.buf[self.zero];
    }

    fn getAfter(&self, offset : usize) -> f32 {
	return self.buf[(self.zero + offset) & self.mask()];
    }

    fn mask(&self) -> usize {
	return self.buf.len();
    }

    fn getBefore(&self, offset : usize) -> f32 {
	let mask = self.mask();
	let minus_one = mask;
	return self.buf[(self.zero + minus_one + 1 - offset) & self.mask()];
    }
}

pub trait HermiteDownsamplerTrait {
    // Downsampling factor
    fn factor(&self) -> usize;

    // Before/after window size
    fn window_size(&self) -> usize;

    // Interpolate from ring buffer of (size window_size * 2) + factor.
    // The zero offset may be within the buffer.
    fn interpolate(&self, buf : &Buf) -> f32;
}

struct Hermite4Pt3rdOrder {}
impl HermiteDownsamplerTrait for Hermite4Pt3rdOrder {
    fn factor(&self) -> usize      { 2 }
    fn window_size(&self) -> usize { 1 }

    fn interpolate(&self, buf : &Buf) -> f32 {
	const x : f32  = 0.5;
	let y_m1 = buf.getBefore(1);
	let y_0 = buf.get();
	let y_p1 = buf.getAfter(1);
	let y_p2 = buf.getAfter(2);

	let c0 = y_0;
	let c1 = 0.5 * (y_p1 - y_m1);
	let c2 = y_m1 - 2.5 * y_0 + 2.0 * y_p1 - 0.5 * y_p2;
	let c3 = 0.5 * (y_p2 - y_m1) + 1.5 * (y_0 - y_p1);
	return ((c3 * x + c2) * x + c1) * x + c0;
    }
}

// ----------------------------------------

pub struct HermiteDownsampler<T> where T : HermiteDownsamplerTrait {
    downsampler : T,
    source : RcSyncWriter,
    buf : Vec<f32>,
}

impl<T> HermiteDownsampler<T>
where T : HermiteDownsamplerTrait
{
}

impl<T> PCMSyncWriter for HermiteDownsampler<T> where T : HermiteDownsamplerTrait {
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult {
//	let result = self.source.borrow_mut().write_sync_pcm(self.buf);
	panic!("TODO");
    }

    fn advance_sync(&mut self, timeslice : Timeslice) {
	self.source.borrow_mut().advance_sync(timeslice);
    }
}

impl<T> FrequencyTrait for HermiteDownsampler<T> where T : HermiteDownsamplerTrait {
    fn frequency(&self) -> crate::audio::Freq {
	return self.source.borrow().frequency() >> self.downsampler.factor();
    }
}

