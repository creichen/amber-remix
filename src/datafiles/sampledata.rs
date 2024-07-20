// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use core::fmt;
use std::ops::Index;

/// PCM sample data (for the entire samples file)
pub struct SampleData {
    pub data : Vec<i8>,
}

impl SampleData {
    pub fn new(data : Vec<u8>) -> SampleData{
	let i8data = data.into_iter().map(|x| x as i8).collect();
	return SampleData { data : i8data };
    }
}

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

impl Into<std::ops::Range<usize>> for &SampleRange {
    fn into(self) -> std::ops::Range<usize> {
        self.start..self.start+self.len
    }
}

impl Into<std::ops::Range<usize>> for SampleRange {
    fn into(self) -> std::ops::Range<usize> {
        self.start..self.start+self.len
    }
}

impl Index<std::ops::Range<usize>> for SampleData {
    type Output = [i8];

    fn index(&self, index: std::ops::Range<usize>) -> &Self::Output {
        &self.data[index]
    }
}

impl Index<&SampleRange> for &SampleData {
    type Output = [i8];

    fn index(&self, index: &SampleRange) -> &Self::Output {
	let r : std::ops::Range<usize> = index.into();
        &self.data[r]
    }
}

impl Index<SampleRange> for SampleData {
    type Output = [i8];

    fn index(&self, index: SampleRange) -> &Self::Output {
	let r : std::ops::Range<usize> = index.into();
        &self.data[r]
    }
}

impl Index<usize> for SampleData {
    type Output = i8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

