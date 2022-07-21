// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use super::{writer::{RcPCMWriter, PCMStereoWriter}, vtracker::TrackerSensor};

const BUF_SIZE : usize = 32;

pub struct StereoMapper {
    left : f32,
    right : f32,
    buf : [f32; BUF_SIZE],
    source : RcPCMWriter,
    tracker : TrackerSensor,
}

impl<'a> StereoMapper {
    pub fn new(left : f32, right : f32, source : RcPCMWriter, tracker : TrackerSensor) -> StereoMapper {
	return StereoMapper {
	    left,
	    right,
	    buf : [0.0; BUF_SIZE],
	    source,
	    tracker,
	};
    }

    pub fn set_volume(&mut self, left : f32, right : f32) {
	self.left = left;
	self.right = right;
    }
}

impl PCMStereoWriter for StereoMapper {
    fn write_stereo_pcm(&mut self, out : &mut [f32]) {
	let mono_samples_requested = out.len() / 2;
	let mut mono_samples_processed = 0;
	let mut out_pos = 0;
	let left_v = self.left;
	let right_v = self.right;
	let mut buf = self.buf;

	let mut debug_total : f32 = 0.0;
	let mut debug_count = 0;
	while mono_samples_processed < mono_samples_requested {

	    let mono_samples_remaining = mono_samples_requested - mono_samples_processed;
	    let len_next_chunk = usize::min(mono_samples_remaining, BUF_SIZE);
	    self.source.borrow_mut().write_pcm(&mut buf[0..len_next_chunk]);

	    let out_end = out_pos + len_next_chunk * 2;
	    let mut buf_pos = 0;

	    debug_count += out_end - out_pos;

	    while out_pos < out_end {
		let sample = buf[buf_pos];
		debug_total += f32::abs(sample);
		self.tracker.add(sample);

		out[out_pos] += left_v * sample;
		out[out_pos + 1] += right_v * sample;
		out_pos += 2;
		buf_pos += 1;
	    }
	    mono_samples_processed += len_next_chunk;

	}
	self.tracker.add_many(debug_total, debug_count);
    }
}
