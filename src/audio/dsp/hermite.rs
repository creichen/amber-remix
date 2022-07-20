// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

// Hermite downsampling
// For downsampling from integer multiples of higher frequencies

use std::{rc::Rc, cell::RefCell};

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
//use crate::util::IndexLen;
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{writer::{PCMWriter, FrequencyTrait, RcPCMWriter}, ringbuf::WindowedBuf};
//use std::{ops::DerefMut, rc::Rc, cell::RefCell};

pub trait HermiteDownsamplerTrait {
    // Downsampling factor
    fn factor(&self) -> usize;

    // Before/after window size
    fn window_size(&self) -> usize;

    // Interpolate from ring buffer of (size window_size * 2) + factor.
    // The zero offset may be within the buffer.
    fn interpolate(&self, buf : &WindowedBuf, offset : usize) -> f32;
}

struct Hermite4Pt3rdOrder {}
impl HermiteDownsamplerTrait for Hermite4Pt3rdOrder {
    fn factor(&self) -> usize      { 2 }
    fn window_size(&self) -> usize { 1 }

    fn interpolate(&self, buf : &WindowedBuf, offset : usize) -> f32 {
	const X : f32  = 0.5;
	let y_m1 = buf.get(offset - 1);
	let y_0 = buf.get(offset);
	let y_p1 = buf.get(offset + 1);
	let y_p2 = buf.get(offset + 2);

	let c0 = y_0;
	let c1 = 0.5 * (y_p1 - y_m1);
	let c2 = y_m1 - 2.5 * y_0 + 2.0 * y_p1 - 0.5 * y_p2;
	let c3 = 0.5 * (y_p2 - y_m1) + 1.5 * (y_0 - y_p1);
	return ((c3 * X + c2) * X + c1) * X + c0;
    }
}

// ----------------------------------------

pub struct HermiteDownsampler<T : 'static> where T : HermiteDownsamplerTrait {
    downsampler : &'static T,
    partial : usize, // last read was partial
    source : RcPCMWriter,
    buf : WindowedBuf,
}

impl<T> HermiteDownsampler<T>
where T : HermiteDownsamplerTrait
{
    fn new(downsampler : &'static T, source : RcPCMWriter) -> HermiteDownsampler<T> {
	return HermiteDownsampler {
	    downsampler,
	    partial : 0,
	    source,
	    buf : WindowedBuf::new(downsampler.window_size() * 2 + downsampler.factor()),
	};
    }

    fn new_rc(downsampler : &'static T, source : RcPCMWriter) -> RcPCMWriter {
	return Rc::new(RefCell::new(HermiteDownsampler::new(downsampler, source)));
    }

    fn prebuf_len(&self) -> usize {
	self.downsampler.window_size() + self.downsampler.factor() - 1
    }
}

const HERMITE4PT3RDORDER : Hermite4Pt3rdOrder = Hermite4Pt3rdOrder{};

pub fn down2x(source : RcPCMWriter) -> RcPCMWriter {
    return HermiteDownsampler::new_rc(&HERMITE4PT3RDORDER, source.clone());
}

impl<T> PCMWriter for HermiteDownsampler<T> where T : HermiteDownsamplerTrait {
    fn write_pcm(&mut self, output : &mut [f32]) {
	let downsample_factor = self.downsampler.factor();
	for o in output.iter_mut() {
	    self.buf.read_pcm(&self.source, downsample_factor - self.partial);
	    *o = self.downsampler.interpolate(&self.buf, self.prebuf_len());
	}
    }
}

impl<T> FrequencyTrait for HermiteDownsampler<T> where T : HermiteDownsamplerTrait {
    fn frequency(&self) -> crate::audio::Freq {
	return self.source.borrow().frequency() >> self.downsampler.factor();
    }
}

