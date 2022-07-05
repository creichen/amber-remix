//use super::dsp::frequency_range::Freq;

const ONE_128TH : f32 = 1.0 / 128.0;

#[derive(Clone, Copy)]
pub struct SampleRange {
    pub start : usize,
    pub len : usize,
}

impl SampleRange {
    pub fn new(start : usize, len : usize) -> SampleRange {
	SampleRange {
	    start, len,
	}
    }
}

pub struct SampleWriter<'a> {
    data : &'a [f32],
    pos : usize,
}

impl<'a> SampleWriter<'a> {
    fn new(data : &'a [f32]) -> SampleWriter<'a> {
	return SampleWriter {
	    data,
	    pos : 0,
	}
    }

    pub fn remaining(&self) -> usize {
	return self.data.len() - self.pos;
    }

    pub fn done(&self) -> bool {
	return self.remaining() == 0;
    }

    pub fn write(&mut self, dest : &mut [f32]) -> usize {
	let max_write = usize::min(dest.len(),
				   self.data.len() - self.pos);
	let data = &self.data;
	let slice = &data[self.pos..self.pos+max_write];
	dest.copy_from_slice(slice);
	self.pos += max_write;
	return max_write;
    }
}

pub trait SampleSource {
    /// Gets the sample that corresponds to the specified sample range.
    fn get_sample<'a>(&'a self, range : SampleRange/*, preferred_freq : Freq*/) -> SampleWriter<'a>;
}

pub struct SimpleSampleSource {
    data : Vec<f32>,
}

impl SimpleSampleSource {
    pub fn new(data : Vec<i8>) -> SimpleSampleSource {
	return SimpleSampleSource {
	    data : data.iter().map(|x| { *x as f32 * ONE_128TH }).collect(),
	};
    }
}

impl SampleSource for SimpleSampleSource {
    fn get_sample<'a>(&'a self, range : SampleRange/*, preferred_freq : Freq*/) -> SampleWriter<'a> {
	let data = &self.data;
	let r = range.start..range.start+range.len;
	return SampleWriter::new(&data[r]);
    }
}
