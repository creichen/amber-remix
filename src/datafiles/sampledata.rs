use std::ops::Range;

/// PCM sample data (for multiple samples)
pub struct SampleData {
    pub data : Vec<i8>,
}

impl SampleData {
    pub fn new(data : Vec<u8>) -> SampleData{
	let i8data = data.into_iter().map(|x| x as i8).collect();
	return SampleData { data : i8data };
    }

    pub fn sample<'a>(&'a self, range : Range<usize>) -> Sample {
	return Sample {
	    sample_data : self,
	    range,
	}
    }
}

/// PCM sample, to be used together with some SampleData
pub struct Sample<'a> {
    pub sample_data : &'a SampleData,
    pub range : Range<usize>,
}


impl<'a> Sample<'a> {
    pub fn as_slice(&self) -> &'a [i8] {
	return &self.sample_data.data[self.range.start..self.range.end];
    }
}
