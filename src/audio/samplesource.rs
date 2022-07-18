// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use core::fmt;
use std::{rc::Rc, collections::hash_map::HashMap, cell::RefCell, time::SystemTime};
use rubato::{Resampler, SincFixedIn, InterpolationType, InterpolationParameters, WindowFunction};

use super::{Freq, amber};

//use super::dsp::frequency_range::Freq;

const ONE_128TH : f32 = 1.0 / 128.0;
const ONE_128TH_F64 : f64 = 1.0 / 128.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleRange {
    pub start : usize,
    pub len : usize,
}

impl SampleRange {
    pub fn at_offset(&self, n : usize) -> SampleRange {
	if n > self.len {
	    SampleRange { start : self.start, len : 0 }
	} else {
	    SampleRange { start : self.start + n, len : self.len - n }
	}
    }
}

impl fmt::Display for SampleRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[0x{:x}..0x{:x} (len=0x{:x} ({}))]",
	       self.start, self.start+self.len,
	       self.len, self.len)
    }
}

impl SampleRange {
    pub fn new(start : usize, len : usize) -> SampleRange {
	SampleRange {
	    start, len,
	}
    }
}

pub struct SampleWriter {
    data : Rc<Vec<f32>>,
    range : SampleRange,
    count : usize,
    freq : Freq,
}

impl SampleWriter {
    fn new(all_data : Rc<Vec<f32>>, range : SampleRange, freq: Freq) -> SampleWriter {
	return SampleWriter {
	    data: all_data,
	    range,
	    count : 0,
	    freq,
	}
    }

    pub fn empty() -> SampleWriter {
	return SampleWriter {
	    data : Rc::new(Vec::new()),
	    range : SampleRange::new(0, 0),
	    count : 0,
	    freq : 300,
	}
    }

    pub fn remaining_secs(&self) -> f64 {
	return self.remaining() as f64 / self.freq as f64;
    }

    pub fn get_freq(&self) -> Freq {
	return self.freq;
    }

    pub fn get_offset(&self) -> usize {
	return self.count;
    }

    /// Forward sample to position OFF_NOMINATOR/OFF_DENOMINATOR
    pub fn forward_to_offset(&mut self, off_nominator : usize, off_denominator : usize) {
	if off_denominator != 0 {
	    let count = (off_nominator * self.data.len()) / off_denominator;
	    if count > self.range.len {
		panic!("Asked to move to location {off_nominator}/{off_denominator}");
	    }
	    self.count = usize::min(count, self.range.len);
	}
    }

    pub fn len(&self) -> usize {
	return self.range.len
    }

    pub fn remaining(&self) -> usize {
	return self.range.len - self.count;
    }

    pub fn done(&self) -> bool {
	return self.remaining() == 0;
    }

    pub fn is_empty(&self) -> bool {
	return self.len() == 0;
    }

    pub fn write(&mut self, dest : &mut [f32]) -> usize {
	let max_write = usize::min(dest.len(),
				   self.remaining());
	let start_pos = self.range.start + self.count;
	let slice = &self.data[start_pos..start_pos+max_write];
	dest.copy_from_slice(slice);
	self.count += max_write;
	return max_write;
    }
}

pub trait SampleSource {
    /// Gets the sample that corresponds to the specified sample RANGE, assuming
    /// that the sample is played at the frequency AT_FREQ .
    fn get_sample(&mut self, range : SampleRange, at_freq : Freq) -> SampleWriter;
}

pub type RcSampleSource = Rc<RefCell<dyn SampleSource>>;


// ----------------------------------------
// Simple sample source; ignores requested frequency

#[derive(Clone)]
pub struct SimpleSampleSource {
    data : Rc<Vec<f32>>,
}

impl SimpleSampleSource {
    // pub fn new(data : Vec<i8>) -> SimpleSampleSource {
    // 	return SimpleSampleSource {
    // 	    data : Rc::new(data.iter().map(|x| { *x as f32 * ONE_128TH }).collect()),
    // 	};
    // }
    pub fn from_iter<'a>(data : std::slice::Iter<'a, i8>) -> SimpleSampleSource {
	return SimpleSampleSource {
	    data : Rc::new(data.map(|x| { *x as f32 * ONE_128TH }).collect()),
	};
    }
    #[cfg(test)]
    pub fn from_vec_f32(data : Vec<f32>) -> SimpleSampleSource {
	return SimpleSampleSource {
	    data : Rc::new(data),
	};
    }
}

impl SampleSource for SimpleSampleSource {
    fn get_sample(&mut self, range : SampleRange, at_freq : Freq) -> SampleWriter {
	return SampleWriter::new(self.data.clone(), range, at_freq);
    }
}


// ----------------------------------------
// Sinc sample source; uses Sinc-based interpolator (from rubato library) for precise match

pub struct SincSampleSource {
    // sample length as index
    cache : HashMap<(usize, usize, usize), Rc<Vec<f32>>>,
    resampler_map : HashMap<usize, RefCell<SincFixedIn<f64>>>,
    base_freq : Freq,
    base_target_freq : f64,
    data : Rc<Vec<f64>>,
}

impl SincSampleSource {
    /// For testig: nw(freq, [((start, len, targetfreq), samples)])
    #[cfg(test)]
    pub fn nw<'a>(out_freq : Freq, data : Vec<((usize, usize, usize), Vec<f32>)>) -> SincSampleSource {
	let mut cache = HashMap::new();
	for (k, v) in data.iter() {
	    cache.insert(*k, Rc::new(v[..].to_vec()));
	}
	return SincSampleSource {
	    cache,
	    base_freq : out_freq,
	    base_target_freq : out_freq as f64,
	    resampler_map : HashMap::new(),
	    data : Rc::new(vec![]),
	}
    }

    pub fn new<'a>(out_freq : Freq, data : Rc<Vec<f64>>) -> SincSampleSource {
	let (min_out_freq, max_out_freq) = amber::get_min_max_freq();
	// These frequencies are for regular notes.  For vibrato, they may be different.

	// For optimal distribution, we want to find "x" and "middle_freq' such that:
	// middle_freq / x = min_out_freq
	// middle_freq * x = max_out_freq
	// => max_out_freq / x = min_out_freq * x
	// => x^2 = max_out_freq / min_out_freq
	let xfact = f64::sqrt(max_out_freq as f64 / min_out_freq as f64);
	let middle_freq = min_out_freq as f64 * xfact;
	println!("Freqs: min:{min_out_freq}..{max_out_freq} -> midlde={middle_freq} with x:{xfact}");


	let mut resampler_map = HashMap::new();

	for size in [64, 366, 2310, 3072, 7168, 10366, 12226] {
	    // let mut sinc_len = 256;
	    let mut sinc_len = 32;
	    while sinc_len > size {
		sinc_len >>= 1;
	    }
	    let params = InterpolationParameters {
		sinc_len,
		f_cutoff: 0.95,
		interpolation: InterpolationType::Linear,
		oversampling_factor: 16,
		window: WindowFunction::BlackmanHarris2,
	    };
	    // let params = InterpolationParameters {
	    // 	sinc_len,
	    // 	f_cutoff: 0.95,
	    // 	interpolation: InterpolationType::Cubic,
	    // 	oversampling_factor: 256,
	    // 	window: WindowFunction::BlackmanHarris2,
	    // };
	    let resampler = SincFixedIn::<f64>::new(
		1.0 / (middle_freq as f64 / out_freq as f64),
		xfact * 1.5, // should leave plenty of room
		params,
		size,
		1,
	    ).unwrap();
	    resampler_map.insert(size, RefCell::new(resampler));
	}

	return SincSampleSource {
	    cache : HashMap::new(),
	    base_freq : out_freq,
	    base_target_freq : middle_freq,
	    resampler_map,
	    data,
	}
    }

    fn get_resampler(&self, desired_freq : Freq, desired_size : usize) -> &RefCell<SincFixedIn<f64>> {
	let resampler = match self.resampler_map.get(&desired_size) {
	    Some(r) => r,
	    None    => panic!("Unsupported size {desired_size}"),
	};
	resampler.borrow_mut().set_resample_ratio_relative(1.0/(desired_freq as f64 / self.base_target_freq)).unwrap();
	return resampler;
    }

    pub fn from_i8(out_freq : Freq, data : &[i8]) -> SincSampleSource {
	return SincSampleSource::new(out_freq, Rc::new(data.as_ref().iter().map(|x| (*x as f64) * ONE_128TH_F64).collect()));
    }

    pub fn from_iter<'a>(out_freq : Freq, data : std::slice::Iter<'a, i8>) -> SincSampleSource {
	let data = Rc::new(data.map(|x| { *x as f64 * (1.0/128.0) }).collect());
	return SincSampleSource::new(out_freq, data);
    }

}

impl SampleSource for SincSampleSource {
    fn get_sample(&mut self, range : SampleRange, at_freq : Freq) -> SampleWriter {
	let sig = (range.start, range.len, at_freq);
	let rdata = match self.cache.get(&sig) {
	    Some(r) => r.clone(),
	    None    => {
		let start = SystemTime::now();
		let resampler = &self.get_resampler(at_freq, range.len);

		let d0 = &self.data[range.start..range.start+range.len];
		let data = [d0];
		let result = resampler.borrow_mut().process(&data, None).unwrap();
		let mut rbuf = vec![];
		for r in &result[0] {
		    rbuf.push(*r as f32);
		}
		let len = rbuf.len();
		let rdata = Rc::new(rbuf);
		let stop = SystemTime::now();
		let duration = stop.duration_since(start).unwrap().as_nanos();
		println!("Resample duration = {:.3} us for {} at {}, outlen={}", duration as f32 / 1000.0, range, at_freq, len);
		self.cache.insert(sig, rdata.clone());
		rdata
	    },
	};
	//let expected_len = range.len * self.base_freq / at_freq;
	// println!("Resapmled to {at_freq}: {} -> {} samples ({expected_len} expected)",
	// 	 d0.len(), rdata.len());
	let len = rdata.len();

	return SampleWriter{
	    data : rdata.clone(),
	    range : SampleRange {
		start : 0,
		len,
	    },
	    count : 0,
	    freq : at_freq,
	};
    }
}

