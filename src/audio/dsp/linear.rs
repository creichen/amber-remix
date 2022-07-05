use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

/// Linearly interpolating remixer
///
/// Not expected to produce particularly high-quality output

use crate::audio::dsp::writer::PCMWriter;
use crate::audio::dsp::writer::FlexPCMWriter;
use crate::audio::dsp::writer::FlexPCMResult;
use super::frequency_range::Freq;
use super::frequency_range::FreqRange;

const BUFFER_SIZE_MILLIS : usize = 100;

pub struct LinearFilter<'a> {
    state : Option<SampleState>,
    max_in_freq : Freq,
    out_freq : Freq,
    buf : Vec<f32>,
    samples_in_buf : usize, // Valid data left in buffer
    source : Rc<RefCell<dyn FlexPCMWriter>>,
    freqs : FreqRange<'a>,
}

impl<'a> LinearFilter<'a> {
    pub fn new(max_in_freq : Freq, out_freq : Freq, source : Rc<RefCell<dyn FlexPCMWriter>>) -> LinearFilter<'a> {
	return LinearFilter {
	    state : None,
	    max_in_freq,
	    out_freq,
	    buf : vec![0.0; (max_in_freq * BUFFER_SIZE_MILLIS) / 1000],
	    samples_in_buf : 0,
	    source,
	    freqs : FreqRange::new(),
	};
    }

    // May underestimate due to rounding
    fn buffer_size_in_seconds(&self) -> f32 {
	let mut pos = 0;
	let mut available = 0.0;
	while pos < self.samples_in_buf {
	    let (freq, freq_remaining) = self.freqs.get(pos);
	    let until_end_of_buf = self.samples_in_buf - pos;
	    let actual_remaining = match freq_remaining {
		None    => until_end_of_buf,
		Some(n) => usize::min(n, until_end_of_buf),
	    };
	    available += actual_remaining as f64 / freq as f64;
	    pos += actual_remaining;
	}
	return available as f32;
    }

    fn fill_buffer(&mut self, output: &mut [f32]) -> FlexPCMResult {
        let requested_output_in_seconds = output.len() as f32 / self.out_freq as f32;
        let available_in_seconds = self.buffer_size_in_seconds();
        let missing_in_seconds = requested_output_in_seconds - available_in_seconds;
        let missing_in_millis = f32::ceil(missing_in_seconds / 1000.0) as usize;
        let buf_offset = self.samples_in_buf;
        let desired_max_to_write = 1 + (missing_in_millis * self.max_in_freq) / 1000;
	let possible_max_to_write = self.buf.len() - buf_offset;
	let max_to_write = usize::min(desired_max_to_write, possible_max_to_write);

        let write_result = {
	    let mut freqs_at_buf_offset = self.freqs.at_offset(buf_offset);
	    self.source.borrow_mut().write_flex_pcm(&mut self.buf[buf_offset..buf_offset+max_to_write], &mut freqs_at_buf_offset,
						    usize::max(1, missing_in_millis))
	};

	if let FlexPCMResult::Wrote(num_written) = write_result {
            if num_written == 0 && max_to_write > 0 && missing_in_millis > 0 {
		if possible_max_to_write == 0 {
		    panic!("LinearFilter buffer too small");
		} else {
		    panic!("Source to LinearFilter refused to provide updates");
		}
	    }

            self.samples_in_buf += num_written;
	}

	// println!("** prep: wrote {num_written}/{max_to_write}, for {missing_in_millis} ms, now have {}", self.samples_in_buf);

	return write_result;
    }

    fn emit_buffer(&mut self, output: &mut [f32]) -> usize {
        let out_len = output.len();
        let mut out_pos = 0;
        let mut in_pos = 0;
        while out_pos < out_len {
	    let out_remaining = out_len - out_pos;
	    let in_remaining = self.samples_in_buf - in_pos;

	    // println!("... onto the next; in: pos@{in_pos}, left:{}", in_remaining);

	    // How much sample information should we write now?
	    let (in_freq, max_in_samples) = self.freqs.get(in_pos);
	    let num_samples_in_per_out = in_freq as f32 / self.out_freq as f32;
	    let max_in_from_sample = match max_in_samples {
		None    => in_remaining, // infinite length -> beyond the size of the output buffer
		Some(l) => l,
	    };

	    // Make sure we have the linear remixer set up
	    let sample_state_last = match self.state {
		// Frequency change?  Invalidate.
		Some(s) => { if s.in_freq == in_freq { Some(s) } else { None } },
		None    => None
	    };
	    let mut sample_state = match sample_state_last {
		Some(s) => s,
		None    => {
		    // println!("!! Need new sample state");
		    SampleState::new(in_freq, self.out_freq)},
	    };

	    // How many samples can we expect to get?
	    let in_from_sample = usize::min(max_in_from_sample,
					    in_remaining - 1);
	    // First double-check that we actually have enough data left
	    if in_from_sample > in_remaining || in_from_sample == 0 {
		break;
	    }

	    let out_from_sample_f32 = (in_from_sample) as f32 / num_samples_in_per_out;
	    let out_from_sample = (out_from_sample_f32 - sample_state.get_pos()) as usize;

	    // println!("-- in@{in_pos} out@{out_pos}");
	    // println!("   freqs={}", self.freqs);
	    // println!("   outbuf=[{out_pos}..{out_len}] -> len={out_remaining}");
	    // println!("   inbuf=[{in_pos}..{in_pos}+{max_in_samples:?}]");
	    // println!("     -> expected max-for-outbuf={out_from_sample}   <- {out_from_sample_f32} - {}", sample_state.get_pos());
	    // println!("        bufsize = {}", self.buf.len());
	    // println!("        expected read: [{in_pos}..{}] (from max {})", in_pos + in_from_sample, in_pos + max_in_from_sample);
	    // println!("        it: {sample_state}");

	    if out_from_sample == 0 {
		break; // Need to get more samples first
	    }

	    if out_remaining > out_from_sample {
		// Sample will finish before / as we fill the output buffer
		// println!("  -> (cont)  [{}..{}] <== [{}..{}]   in->out rate = {num_samples_in_per_out}",
		// 	 out_pos, out_pos+out_from_sample,
		// 	 in_pos, in_pos+in_from_sample);

		sample_state.resample(&mut output[out_pos..out_pos+out_from_sample],
				      // +1 so that we can interpolate to the next sample:
				      &self.buf[in_pos..in_pos+in_from_sample + 1]);
		//in_pos += in_from_sample;
		out_pos += out_from_sample;
		// if Some(in_from_sample) == max_in_samples {
		//     println!("!! Reset sample state (1)");
		//     self.state = None;
		// } else {
		//     self.state = Some(sample_state);
		// }

	    } else {
		// We will fill the output buffer before the sample is done
		// println!("  -> (finl)  [{}..{}] <== [{}..{}]   in->out rate = {num_samples_in_per_out}",
		// 	 out_pos, out_pos+out_from_sample,
		// 	 in_pos, in_pos+in_from_sample);
		sample_state.resample(&mut output[out_pos..out_len],
				      // +1 so that we can interpolate to the next sample:
				      &self.buf[in_pos..in_pos+in_from_sample + 1]);
		// println!("        -> it': {sample_state}");

		out_pos = out_len;

		// move int offset in sapmler state back to main object so that we can flush more data
		//println!("           resetting state? {in_progress} >= {in_from_sample}?");
		// if Some(in_progress) == max_in_samples {
		//     println!("!! Reset sample state (2)");
		//     self.state = None;
		// } else {
		//     // Store sample_state for the next time we are called
		//     self.state = Some(sample_state);
		// }
	    }
	    let in_progress = sample_state.sample_pos_int;
	    sample_state.sample_pos_int = 0;
	    in_pos += in_progress;

	    self.state = Some(sample_state);
	}
	// println!("  cleanup: in_pos = {in_pos}");
        self.freqs.shift(in_pos);
        let left_over_samples = in_pos..self.samples_in_buf;
        self.samples_in_buf = left_over_samples.len();
        self.buf.copy_within(left_over_samples, 0);
        return out_pos;
    }

}

impl<'a> PCMWriter for LinearFilter<'a> {
    fn frequency(&self) -> Freq {
	return self.out_freq;
    }

    fn write_pcm(&mut self, output : &mut [f32]) {
	let output_requested = output.len();
	let mut output_written = 0;
	let mut conclude_with_silence = false;
	while output_written < output_requested {
	    let mut num_read = 0;
	    loop {
		match self.fill_buffer(output) {
		    FlexPCMResult::Wrote(r) => {
			num_read = r;
			break;
		    },
		    FlexPCMResult::Flush => {
			self.samples_in_buf = 0;
			self.state = None;
			continue;
		    }
		    FlexPCMResult::Silence => {
			conclude_with_silence = true;
			break;
		    }
		}
	    };
	    // println!("[TOP]  buf = {:?}", &self.buf[..self.samples_in_buf]);
	    // println!("[TOP]  out = {:?}", &output[..output_written]);
	    // println!("[TOP]  after {num_read} reads: requesting write at: {output_written}/{output_requested} with {}/{} samples", self.samples_in_buf, self.buf.len());
	    let num_written = self.emit_buffer(&mut output[output_written..]);

	    output_written += num_written;
	    if conclude_with_silence {
		return;
	    }
	    // println!("[TOP]  TOTAL PROGRESS: {output_written}/{output_requested} with {} samples", self.samples_in_buf);
	    if num_read == 0 && num_written == 0 {
		panic!("No progress in linear filter: buf {}/{}", self.samples_in_buf, self.buf.len());
	    }
        }
    }

}


#[derive(Copy, Clone)]
struct SampleState {
    in_freq : Freq,
    out_freq : Freq,         // output buffer frequency

    // index into sample data
    sample_pos_int : usize,   // integral part
    sample_pos_fract : f32, // fractional part (nominator; the denominator is out_freq)
}

impl SampleState {
    fn new(in_freq : Freq, out_freq : Freq) -> SampleState {
	SampleState {
	    in_freq,
	    out_freq,
	    sample_pos_int : 0,
	    sample_pos_fract : 0.0,
	}
    }

    fn get_pos(&self) -> f32 {
	return self.sample_pos_int as f32 + (self.sample_pos_fract / self.out_freq as f32);
    }

    fn resample(&mut self, outbuf : &mut [f32], inbuf : &[f32]) {
	let sample_len = inbuf.len();
	// println!("  ## resamp from {}", inbuf[0]);
	let mut pos = self.sample_pos_int;

	// fractional position counter
	let mut fpos_nom = self.sample_pos_fract as f32;
	let fpos_nom_inc_total = self.in_freq;
	let fpos_denom = self.out_freq as f32;
	let pos_inc = (fpos_nom_inc_total / self.out_freq) as usize;
	let fpos_nom_inc = (fpos_nom_inc_total % self.out_freq) as f32;

	for out in outbuf.iter_mut() {
	    // Linear interpolation
	    let sample_v_current = inbuf[pos];

	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { inbuf[pos + 1] };

	    let sample_v_current_fragment = sample_v_current * (fpos_denom - fpos_nom);
	    let sample_v_next_fragment = sample_v_next * fpos_nom;

	    // println!("  ## interpol {}, {}", sample_v_current, sample_v_next);

	    let sample_v = (sample_v_current_fragment + sample_v_next_fragment) / fpos_denom;

	    *out += sample_v;

	    pos += pos_inc;
	    fpos_nom = fpos_nom + fpos_nom_inc;
	    if fpos_nom >= fpos_denom {
		fpos_nom -= fpos_denom;
		pos += 1;
	    }
	}
	self.sample_pos_int = pos;
	self.sample_pos_fract = fpos_nom;
    }
}

impl Display for SampleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	write!(f, "SampleState{{ in:{} Hz -> out:{} Hz; pos : {} + {}/{} }}",
	       self.in_freq, self.out_freq,
	       self.sample_pos_int, self.sample_pos_fract, self.out_freq)
    }
}

// ----------------------------------------

#[cfg(test)]
#[test]
fn test_copy() {
    let mut outbuf : [f32; 5] = [0.0; 5];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 20.0,
		 100.0,
		 10.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_double() {
    let mut outbuf : [f32; 10] = [0.0; 10];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 200);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 12.5,
		 20.0,
		 60.0,
		 100.0,
		 55.0,
		 10.0,
		 25.0,
		 40.0,
		 40.0,],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_triple() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_upsample_incremental() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
		 &outbuf[..]);

    sstate.resample(&mut outbuf[2..11], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0 ],
		 &outbuf[..]);

    sstate.resample(&mut outbuf[11..15], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_double() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(100, 50);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..3], &inbuf);
    assert_eq!( [10.0,
		 30.0,
		 50.0,
		 0.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_ten() {
    let mut outbuf : [f32; 10] = [0.0; 10];
    let inbuf = vec![1.0, 2.0, 3.0];
    let mut sstate = SampleState::new(10, 100);
    sstate.resample(&mut outbuf[0..10], &inbuf);
    assert_eq!( [1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_one_point_five() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_downsample_one_point_five_incremental() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 0.0,
		 0.0 ],
		 &outbuf[..]);
    sstate.resample(&mut outbuf[2..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
struct MockFlexWriter { s : Vec<f32>, f : Vec<(usize, Freq)>, maxwrite : usize }
#[cfg(test)]
impl FlexPCMWriter for MockFlexWriter {
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange, _msecs : usize) -> FlexPCMResult {
	let maxsize = usize::min(self.maxwrite, usize::min(output.len(), self.s.len()));
	output[0..maxsize].copy_from_slice(&self.s[0..maxsize]);
	let f = &self.f;
	for (pos, freq) in f {
	    freqrange.append(*pos, *freq);
	}
	self.f = vec![];
	self.s.copy_within(maxsize.., 0);
	return FlexPCMResult::Wrote(maxsize);
    }
}

#[cfg(test)]
#[test]
fn test_linear_filter_resampling_incremental() {
    let mut outbuf = [0.0; 14];
    let mut flexwriter = MockFlexWriter {
	maxwrite : 100,
	s : vec![1.0, 2.0,                           // 1:1
		 3.0, 4.0, 5.0, 6.0,                 // 2:1 (downsample)
		 7.0, 8.0, 9.0,                      // 1:2 (upsample)
		 10.0, 20.0, 30.0, 40.0, 50.0, 60.0  // 1.5:1 (downsample)
	],
	f : vec![(0, 10000), (2, 20000), (6, 5000), (9, 15000)],
    };
    let mut lf = LinearFilter::new(20000, 10000, Rc::new(RefCell::new(flexwriter)));
    lf.write_pcm(&mut outbuf[0..1]);
    assert_eq!( [1.0,
		 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    lf.write_pcm(&mut outbuf[1..4]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    lf.write_pcm(&mut outbuf[4..5]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0,
		 0.0, 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    lf.write_pcm(&mut outbuf[5..6]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5,
		 0.0, 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);
    lf.write_pcm(&mut outbuf[6..7]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0,
		 0.0,
		 0.0, 0.0, 0.0, 0.0,
		 0.0, 0.0,
		 ],
		 &outbuf[..]);

    lf.write_pcm(&mut outbuf[7..11]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0,
		 0.0, 0.0, 0.0,
		 ],
		 &outbuf[..]);

    lf.write_pcm(&mut outbuf[11..13]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0, 25.0, 40.0,
		 0.0,
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_linear_filter_resampling() {
    let mut outbuf = [0.0; 14];
    let mut flexwriter = MockFlexWriter {
	maxwrite : 100,
	s : vec![1.0, 2.0,                           // 1:1
		 3.0, 4.0, 5.0, 6.0,                 // 2:1 (downsample)
		 7.0, 8.0, 9.0,                      // 1:2 (upsample)
		 10.0, 20.0, 30.0, 40.0, 50.0, 60.0  // 1.5:1 (downsample)
	],
	f : vec![(0, 10000), (2, 20000), (6, 5000), (9, 15000)],
    };
    let mut lf = LinearFilter::new(20000, 10000, Rc::new(RefCell::new(flexwriter)));
    lf.write_pcm(&mut outbuf[..]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0, 25.0, 40.0, 55.0,
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn test_linear_filter_limit_writes() {
    for i in 1..3 {
	let mut outbuf = [0.0; 14];
	let mut flexwriter = MockFlexWriter {
	    maxwrite : i,
	    s : vec![1.0, 2.0,                           // 1:1
		     3.0, 4.0, 5.0, 6.0,                 // 2:1 (downsample)
		     7.0, 8.0, 9.0,                      // 1:2 (upsample)
		     10.0, 20.0, 30.0, 40.0, 50.0, 60.0  // 1.5:1 (downsample)
	    ],
	    f : vec![(0, 10000), (2, 20000), (6, 5000), (9, 15000)],
	};
	let mut lf = LinearFilter::new(20000, 10000, Rc::new(RefCell::new(flexwriter)));
	lf.write_pcm(&mut outbuf[..]);
	assert_eq!( [1.0, 2.0,
		     3.0, 5.0,
		     7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		     10.0, 25.0, 40.0, 55.0,],
		     &outbuf[..]);
    }
}

#[cfg(test)]
#[test]
fn test_linear_filter_tiny_buffer() {
    let mut outbuf = [0.0; 14];
    let mut flexwriter = MockFlexWriter {
	maxwrite : 1000,
	s : vec![1.0, 2.0,                           // 1:1
		 3.0, 4.0, 5.0, 6.0,                 // 2:1 (downsample)
		 7.0, 8.0, 9.0,                      // 1:2 (upsample)
		 10.0, 20.0, 30.0, 40.0, 50.0, 60.0  // 1.5:1 (downsample)
	],
	f : vec![(0, 40), (2, 80), (6, 20), (9, 60)],
    };
    let mut lf = LinearFilter::new(80, 40, Rc::new(RefCell::new(flexwriter)));
    lf.write_pcm(&mut outbuf[..]);
    assert_eq!( [1.0, 2.0,
		 3.0, 5.0,
		 7.0, 7.5, 8.0, 8.5, 9.0, 9.5,
		 10.0, 25.0, 40.0, 55.0,],
		 &outbuf[..]);
}

// ================================================================================
