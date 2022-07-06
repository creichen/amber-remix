use core::fmt;
use std::rc::Rc;

//use super::dsp::frequency_range::Freq;

const ONE_128TH : f32 = 1.0 / 128.0;

#[derive(Clone, Copy, Debug)]
pub struct SampleRange {
    pub start : usize,
    pub len : usize,
}

impl fmt::Display for SampleRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, " samplerange[0x{:x}..0x{:x} ({}..{}) (len=0x{:x} ({}))]",
	       self.start, self.start+self.len,
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
}

impl SampleWriter {
    fn new(all_data : Rc<Vec<f32>>, range : SampleRange) -> SampleWriter {
	return SampleWriter {
	    data: all_data,
	    range,
	    count : 0,
	}
    }

    pub fn empty() -> SampleWriter {
	return SampleWriter {
	    data : Rc::new(Vec::new()),
	    range : SampleRange::new(0, 0),
	    count : 0,
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
    /// Gets the sample that corresponds to the specified sample range.
    fn get_sample(&self, range : SampleRange/*, preferred_freq : Freq*/) -> SampleWriter;
}

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
    fn get_sample(&self, range : SampleRange/*, preferred_freq : Freq*/) -> SampleWriter {
	return SampleWriter::new(self.data.clone(), range);
    }
}
