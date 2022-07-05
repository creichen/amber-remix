use super::writer::PCMWriter;

const BUF_SIZE : usize = 32;

pub struct StereoMapper<'a> {
    left : f32,
    right : f32,
    buf : [f32; BUF_SIZE],
    source : &'a mut dyn PCMWriter,
}

impl<'a> StereoMapper<'a> {
    pub fn new(left : f32, right : f32, source : &'a mut dyn PCMWriter) -> StereoMapper<'a> {
	return StereoMapper {
	    left,
	    right,
	    buf : [0.0; BUF_SIZE],
	    source,
	};
    }

    pub fn set_volume(&mut self, left : f32, right : f32) {
	self.left = left;
	self.right = right;
    }

    pub fn write_stereo_pcm(&mut self, out : &mut [f32]) {
	let mono_samples_requested = out.len() / 2;
	let mut mono_samples_processed = 0;
	let mut out_pos = 0;
	let left_v = self.left;
	let right_v = self.right;
	let mut buf = self.buf;

	while mono_samples_processed < mono_samples_requested {
	    let mono_samples_remaining = mono_samples_requested - mono_samples_processed;
	    let len_next_chunk = usize::min(mono_samples_remaining, BUF_SIZE);
	    self.source.write_pcm(&mut buf[0..len_next_chunk]);

	    let out_end = out_pos + len_next_chunk * 2;
	    let mut buf_pos = 0;
	    while out_pos < out_end {
		let sample = buf[buf_pos];
		out[out_pos] = left_v * sample;
		out[out_pos + 1] = right_v * sample;
		out_pos += 2;
		buf_pos += 1;
	    }
	    mono_samples_processed += len_next_chunk;
	}
    }
}
