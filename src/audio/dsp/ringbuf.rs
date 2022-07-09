#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use crate::util::IndexLen;

const OUTPUT_BUFFER_IS_FULL : usize = 0xffffffff;

/// Ring buffer
pub struct RingBuf {
    last_poll : usize,
    write_pos : usize,
    read_pos : usize,
    data : Vec<f32>,
}

impl RingBuf {
    pub fn new(size : usize) -> RingBuf {
	RingBuf {
	    last_poll : size,
	    write_pos : 0,
	    read_pos : 0,
	    data : vec![0.0; size],
	}
    }

    pub fn internal(&self) -> String {
	return format!("{{ rd:{:x} wr:{:x} len:{:x} }}", self.read_pos, self.write_pos, self.len());
    }

    pub fn capacity(&self) -> usize {
	return self.data.len();
    }

    // Shrink buffer contents to size 0
    pub fn reset(&mut self) {
	self.write_pos = 0;
	self.read_pos = 0;
    }

    pub fn remaining_capacity(&self) -> usize {
	return self.capacity() - self.len();
    }

    pub fn is_full(&self) -> bool {
	return self.write_pos == OUTPUT_BUFFER_IS_FULL;
    }

    pub fn is_empty(&self) -> bool {
	return self.len() == 0;
    }

    pub fn len(&self) -> usize {
	let cap = self.capacity();
	if self.is_full() {
	    return cap;
	};
	if self.write_pos < self.read_pos {
	    return self.write_pos + cap - self.read_pos;
	}
	return self.write_pos - self.read_pos;
    }

    /// How much are we expecting to read from here?
    pub fn expected_read(&self) -> usize {
	return self.last_poll;
    }

    fn can_read_to_end_of_buffer(&self) -> bool{
	return self.is_full() || self.read_pos > self.write_pos;
    }

    /// push_back
    pub fn write_to(&mut self, dest : &mut [f32]) -> usize {
	self.last_poll = dest.len();
	self._write_to(dest)
    }

    fn available_to_read(&self) -> usize {
	let read_end_pos = if self.can_read_to_end_of_buffer() { self.capacity() } else { self.write_pos };
	return read_end_pos - self.read_pos;
    }

    fn available_to_write(&mut self) -> usize {
        let write_end_pos = if self.read_pos <= self.write_pos { self.capacity() } else { self.read_pos };
        let avail = write_end_pos - self.write_pos;
        return avail;
    }

    fn _advance_write_pos(&mut self, amount : usize) {
	self.write_pos += amount;
	if self.write_pos == self.data.len() {
	    self.write_pos = 0;
	}
	if self.write_pos == self.read_pos {
	    self.write_pos = OUTPUT_BUFFER_IS_FULL;
	}
    }

    fn _write_to(&mut self, dest : &mut [f32]) -> usize {
	let initially_available = self.len();
	let requested = dest.len();
	let avail = self.available_to_read();

	let to_write = usize::min(avail, requested);

	if to_write > 0 {
	    dest[0..to_write].copy_from_slice(&self.data[self.read_pos..self.read_pos + to_write]);

	    if self.is_full() {
		self.write_pos = self.read_pos;
	    }
	    self.read_pos += to_write;
	}
	// We might be done now
	if to_write == requested || to_write == initially_available {
	    if self.read_pos == self.data.len() {
		self.read_pos = 0;
	    }

	    return to_write;
	}
	// Otherwise, we must have hit the end of the buffer
	// Call ourselves one final time to finish up
	self.read_pos -= self.capacity();
	return to_write + self._write_to(&mut dest[to_write..]);
    }

    /// Remove the specified number of most recently added samples
    /// Return how many were actually removed
    pub fn drop_back(&mut self, to_remove : usize) -> Result<usize, String> {
	if to_remove > self.len() {
	    return Err(format!("Insufficient capacity: requested drop_back({to_remove}) on only {} elements", self.len()));
	}
	if to_remove == 0 {
	    return Ok(0);
	}
	let initial_write_pos =
	    if self.write_pos == OUTPUT_BUFFER_IS_FULL { self.read_pos } else { self.write_pos };
	if to_remove <= initial_write_pos {
	    self.write_pos = initial_write_pos - to_remove;
	} else {
	    self.write_pos = self.capacity() + initial_write_pos - to_remove;
	}
	return Ok(to_remove);
    }

    /// pop_front()
    pub fn read_from(&mut self, src : &[f32]) -> usize {
	if self.is_full() {
	    return 0;
	}
	let initially_available = self.capacity() - self.len();
	let requested = src.len();
        let write_start_pos = self.write_pos;
	let avail = self.available_to_write();

	let to_write = usize::min(avail, requested);

	if to_write > 0 {
	    self.data[write_start_pos..write_start_pos+to_write].copy_from_slice(&src[0..to_write]);

	    self._advance_write_pos(to_write);
	}
	// We might be done now
	if to_write == requested || to_write == initially_available {
	    return to_write;
	}
	// Otherwise, we must have hit the end of the buffer
	// Call ourselves one final time to finish up
	return to_write + self.read_from(&src[to_write..]);
    }

    /// Remove the specified number of samples that would be next to be read
    /// Return how many were actually removed
    pub fn drop_front(&mut self, to_remove : usize) -> Result<usize, String> {
	if to_remove > self.len() {
	    return Err(format!("Insufficient capacity: requested drop_front({to_remove}) on only {} elements", self.len()));
	}
	if to_remove == 0 {
	    return Ok(0);
	}
	if self.write_pos == OUTPUT_BUFFER_IS_FULL {
	    self.write_pos = self.read_pos;
	}
	let bufsize = self.data.len();

	self.read_pos += to_remove;

	if self.read_pos >= bufsize {
	    self.read_pos -= bufsize
	}
	return Ok(to_remove);
    }

    /// Request write access directly into this buffer.  Note that the window may be smaller than requested
    /// even if the buffer has more capacity than requested; in that case; make sure to iterate.
    /// The returned slice is considered to be filled afterwards.
    pub fn wrbuf<'a>(&'a mut self, size : usize) -> &'a mut [f32] {
	if self.is_full() {
	    return &mut self.data[0..0];
	}
	let avail = self.available_to_write();
	let start_pos = self.write_pos;
	let end_pos = self.write_pos + usize::min(size, avail);
	self._advance_write_pos(end_pos - start_pos);
	return &mut self.data[start_pos..end_pos];
    }

    /// Retrieves an indexed expression for reading out of a slice into the buffer
    /// Does not drop any values.
    pub fn peek_front<'a>(&'a self, size : usize) -> RingBufIndex<'a> {
	let size = usize::min(size, self.len());
	return RingBufIndex {
	    buf : &self,
	    size,
	}
    }
}

pub struct RingBufIndex<'a> {
    buf : &'a RingBuf,
    size : usize,
}

impl<'a> IndexLen<f32> for RingBufIndex<'a> {
    fn len(&self) -> usize {
	return self.size;
    }

    fn get(&self, index: usize) -> f32 {
	if index >= self.size {
	    panic!("Out of bounds access: {index} >= {}", self.size);
	}
	let buflen = self.buf.data.len();
	let pos = index + self.buf.read_pos;
	if pos >= buflen {
	    return self.buf.data[pos - buflen];
	} else {
	    return self.buf.data[pos];
	}
    }
}

// ========================================
// Testing

// ----------------------------------------
// Helpers

#[cfg(test)]
fn assert_empty(b : &mut RingBuf) {
    let mut data = [3.0; 3];

    assert_eq!(b.capacity(), b.remaining_capacity());
    assert_eq!(false, b.is_full());
    assert_eq!(true, b.is_empty());
    assert_eq!(0, b.len());

    assert_eq!(0, b.write_to(&mut data[..]));
    assert_eq!([3.0, 3.0, 3.0],
	       &data[..]);

    assert_eq!(b.capacity(), b.remaining_capacity());
    assert_eq!(false, b.is_full());
    assert_eq!(true, b.is_empty());
    assert_eq!(0, b.len());
}

#[cfg(test)]
fn assert_full(b : &mut RingBuf) {
    let mut data = [3.0; 3];

    assert_eq!(0, b.remaining_capacity());
    assert_eq!(true, b.is_full());
    assert_eq!(false, b.is_empty());
    assert_eq!(b.len() + b.remaining_capacity(), b.capacity());

    assert_eq!(0, b.read_from(&mut data[..]));
    assert_eq!(0, b.remaining_capacity());
    assert_eq!(true, b.is_full());
    assert_eq!(false, b.is_empty());
    assert_eq!(b.len() + b.remaining_capacity(), b.capacity());
    assert_eq!(0, b.wrbuf(1).len());
}

#[cfg(test)]
fn assert_partially_filled(b : &mut RingBuf, size : usize) {
    assert_eq!(b.capacity(), size + b.remaining_capacity());
    assert_eq!(false, b.is_full());
    assert_eq!(false, b.is_empty());
    assert_eq!(size, b.len());
}

// ----------------------------------------
// Tests begin here

#[cfg(test)]
#[test]
fn test_empty() {
    let mut b = RingBuf::new(7);
    assert_eq!(7, b.capacity());
    assert_empty(&mut b);
}

#[cfg(test)]
#[test]
fn test_basic_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_empty(&mut b);

    assert_eq!([1.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_double_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_partially_filled(&mut b, 1);
    assert_eq!(1, b.read_from(&data1[1..2]));
    assert_partially_filled(&mut b, 2);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_partially_filled(& mut b, 1);
    assert_eq!(1, b.write_to(&mut data2[1..2]));
    assert_empty(&mut b);

    assert_eq!([1.0, 2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_interleaved_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(4);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_partially_filled(&mut b, 1);
    assert_eq!(2, b.read_from(&data1[1..3]));
    assert_partially_filled(&mut b, 3);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_partially_filled(& mut b, 1);
    assert_eq!(1, b.write_to(&mut data2[2..3]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 3.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_full_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 3.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_cross_boundary_full_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0, 5.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(3);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_empty(& mut b);

    assert_eq!(3, b.read_from(&data1[1..4]));
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([2.0, 3.0, 4.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_cross_boundary_partial_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_eq!(2, b.read_from(&data1[2..4]));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_eq!([3.0, 4.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_overfull_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(2);

    assert_eq!(2, b.read_from(&data1[0..3]));
    assert_full(&mut b);

    assert_eq!(2, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_overfull_cross_boundary_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let mut data2 = [0.0; 7];
    let mut b = RingBuf::new(3);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_eq!(3, b.read_from(&data1[2..6]));
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[3..]));
    assert_empty(& mut b);

    assert_eq!([3.0, 4.0, 5.0, 0.0],
	       &data2[3..]);
}


// ----------------------------------------
// windowed writes

#[cfg(test)]
fn assert_windowed_write(b : &mut RingBuf, src : &[f32], len_expected : usize) {
    let w = b.wrbuf(src.len());
    assert_eq!(len_expected, w.len());
    w.copy_from_slice(&src[..len_expected]);
}

#[cfg(test)]
#[test]
fn test_windowed_basic_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..1], 1);
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_empty(& mut b);

    assert_eq!([1.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_double_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..1], 1);
    assert_partially_filled(&mut b, 1);
    assert_windowed_write(&mut b, &data1[1..2], 1);
    assert_partially_filled(&mut b, 2);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_partially_filled(& mut b, 1);
    assert_eq!(1, b.write_to(&mut data2[1..2]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_interleaved_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(4);

    assert_windowed_write(&mut b, &data1[0..1], 1);
    assert_partially_filled(&mut b, 1);
    assert_windowed_write(&mut b, &data1[1..3], 2);
    assert_partially_filled(&mut b, 3);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_partially_filled(& mut b, 1);
    assert_eq!(1, b.write_to(&mut data2[2..3]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 3.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_full_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..3], 3);
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 3.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_cross_boundary_full_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0, 5.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..1], 1);
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_empty(& mut b);

    assert_windowed_write(&mut b, &data1[1..4], 2);
    assert_partially_filled(&mut b, 2);
    assert_windowed_write(&mut b, &data1[3..4], 1);
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([2.0, 3.0, 4.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_cross_boundary_partial_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..2], 2);
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_windowed_write(&mut b, &data1[2..4], 1);
    assert_partially_filled(&mut b, 1);
    assert_windowed_write(&mut b, &data1[3..4], 1);
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_eq!([3.0, 4.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_overfull_write_read() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(2);

    assert_windowed_write(&mut b, &data1[0..3], 2);
    assert_full(&mut b);

    assert_eq!(2, b.write_to(&mut data2[..]));
    assert_empty(& mut b);

    assert_eq!([1.0, 2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_windowed_overfull_cross_boundary_write_read() {
    let data1 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let mut data2 = [0.0; 7];
    let mut b = RingBuf::new(3);

    assert_windowed_write(&mut b, &data1[0..2], 2);
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_windowed_write(&mut b, &data1[2..], 1);
    assert_partially_filled(&mut b, 1);
    assert_windowed_write(&mut b, &data1[3..], 2);
    assert_full(&mut b);
    assert_eq!(0, b.read_from(&data1[0..]));
    assert_full(&mut b);

    assert_eq!(3, b.write_to(&mut data2[3..]));
    assert_empty(& mut b);

    assert_eq!([3.0, 4.0, 5.0, 0.0],
	       &data2[3..]);
}

// --------------------
// reset

#[cfg(test)]
#[test]
fn test_reset_empty() {
    let mut b = RingBuf::new(7);
    assert_eq!(7, b.capacity());
    assert_empty(&mut b);
    b.reset();
    assert_empty(&mut b);
}

#[cfg(test)]
#[test]
fn test_reset_partially_full() {
    let data1 = [1.0, 2.0, 3.0];
    let mut b = RingBuf::new(3);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_partially_filled(&mut b, 1);
    b.reset();
    assert_empty(&mut b);
}

#[cfg(test)]
#[test]
fn test_reset_direct_full() {
    let data1 = [1.0, 2.0, 3.0];
    let mut b = RingBuf::new(3);

    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);
    b.reset();
    assert_empty(&mut b);
}

#[cfg(test)]
#[test]
fn test_reset_overlap_full() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 3];
    let mut b = RingBuf::new(3);

    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_eq!(1, b.write_to(&mut data2[0..3]));
    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);
    b.reset();
    assert_empty(&mut b);
}

// --------------------
// drop_back

#[cfg(test)]
#[test]
fn test_drop_back_empty() {
    let mut b = RingBuf::new(7);
    assert_eq!(7, b.capacity());
    if let Ok(_) = b.drop_back(1) {
	panic!("Should not be able to drop_back");
    }
}

#[cfg(test)]
#[test]
fn test_drop_back_partially_full() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(5);

    assert_eq!(4, b.read_from(&data1[0..4]));
    assert_partially_filled(&mut b, 4);
    assert_eq!(Ok(2), b.drop_back(2));
    assert_partially_filled(&mut b, 2);
    assert_eq!(2, b.write_to(&mut data2));
    assert_empty(&mut b);

    assert_eq!([1.0, 2.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_drop_back_direct_full() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 2];
    let mut b = RingBuf::new(3);

    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);
    assert_eq!(Ok(1), b.drop_back(1));
    assert_partially_filled(&mut b, 2);
    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_partially_filled(&mut b, 1);
    assert_eq!(Ok(1), b.drop_back(1));
    assert_empty(&mut b);

    assert_eq!([1.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
fn test_boundary_drop_back_2(capacity : usize) {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut data3 = [0.0; 3];
    let mut b = RingBuf::new(capacity);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_eq!(2, b.write_to(&mut data3[0..3]));
    assert_empty(&mut b);

    assert_eq!(4, b.read_from(&data1[0..4]));
    if capacity == 4 {
	assert_full(&mut b);
    } else {
	assert_partially_filled(&mut b, 4);
    }
    assert_eq!(Ok(2), b.drop_back(2));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2));
    assert_empty(&mut b);
    assert_eq!(Ok(0), b.drop_back(0));

    assert_eq!([1.0, 2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
fn test_boundary_drop_back_3(capacity : usize) {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut data3 = [0.0; 3];
    let mut b = RingBuf::new(capacity);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_eq!(2, b.write_to(&mut data3[0..3]));
    assert_empty(&mut b);

    assert_eq!(4, b.read_from(&data1[0..4]));
    if capacity == 4 {
	assert_full(&mut b);
    } else {
	assert_partially_filled(&mut b, 4);
    }
    assert_eq!(Ok(3), b.drop_back(3));
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2));
    assert_empty(&mut b);
    assert_eq!(Ok(0), b.drop_back(0));

    assert_eq!([1.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_drop_back_partial_overlap_touch_boundaries() {
    test_boundary_drop_back_2(5);
}

#[cfg(test)]
#[test]
fn test_drop_back_partial_overlap_cross_boundaries() {
    test_boundary_drop_back_3(5);
}

#[cfg(test)]
#[test]
fn test_drop_back_full_overlap_touch_boundaries() {
    test_boundary_drop_back_2(4);
}

#[cfg(test)]
#[test]
fn test_drop_back_full_overlap_cross_boundaries() {
    test_boundary_drop_back_3(4);
}

// --------------------
// drop_front

#[cfg(test)]
#[test]
fn test_drop_front_empty() {
    let mut b = RingBuf::new(7);
    assert_eq!(7, b.capacity());
    if let Ok(_) = b.drop_front(1) {
	panic!("Should not be able to drop_front");
    }
}

#[cfg(test)]
#[test]
fn test_drop_front_partially_full() {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(5);

    assert_eq!(4, b.read_from(&data1[0..4]));
    assert_partially_filled(&mut b, 4);
    assert_eq!(Ok(2), b.drop_front(2));
    assert_partially_filled(&mut b, 2);
    assert_eq!(2, b.write_to(&mut data2));
    assert_empty(&mut b);

    assert_eq!([3.0, 4.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_drop_front_direct_full() {
    let data1 = [1.0, 2.0, 3.0];
    let mut data2 = [0.0; 2];
    let mut b = RingBuf::new(3);

    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);
    assert_eq!(Ok(1), b.drop_front(1));
    assert_partially_filled(&mut b, 2);
    assert_eq!(1, b.write_to(&mut data2[0..1]));
    assert_partially_filled(&mut b, 1);
    assert_eq!(Ok(1), b.drop_front(1));
    assert_empty(&mut b);

    assert_eq!([2.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
fn test_boundary_drop_front_2(capacity : usize) {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut data3 = [0.0; 3];
    let mut b = RingBuf::new(capacity);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_eq!(2, b.write_to(&mut data3[0..3]));
    assert_empty(&mut b);

    assert_eq!(4, b.read_from(&data1[0..4]));
    if capacity == 4 {
	assert_full(&mut b);
    } else {
	assert_partially_filled(&mut b, 4);
    }
    assert_eq!(Ok(2), b.drop_front(2));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2));
    assert_empty(&mut b);
    assert_eq!(Ok(0), b.drop_front(0));

    assert_eq!([3.0, 4.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
fn test_boundary_drop_front_3(capacity : usize) {
    let data1 = [1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 3];
    let mut data3 = [0.0; 3];
    let mut b = RingBuf::new(capacity);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_eq!(2, b.write_to(&mut data3[0..3]));
    assert_empty(&mut b);

    assert_eq!(4, b.read_from(&data1[0..4]));
    if capacity == 4 {
	assert_full(&mut b);
    } else {
	assert_partially_filled(&mut b, 4);
    }
    assert_eq!(Ok(3), b.drop_front(3));
    assert_partially_filled(&mut b, 1);

    assert_eq!(1, b.write_to(&mut data2));
    assert_empty(&mut b);
    assert_eq!(Ok(0), b.drop_front(0));

    assert_eq!([4.0, 0.0, 0.0],
	       &data2[..]);
}

#[cfg(test)]
#[test]
fn test_drop_front_partial_overlap_touch_boundaries() {
    test_boundary_drop_front_2(5);
}

#[cfg(test)]
#[test]
fn test_drop_front_partial_overlap_cross_boundaries() {
    test_boundary_drop_front_3(5);
}

#[cfg(test)]
#[test]
fn test_drop_front_full_overlap_touch_boundaries() {
    test_boundary_drop_front_2(4);
}

#[cfg(test)]
#[test]
fn test_drop_front_full_overlap_cross_boundaries() {
    test_boundary_drop_front_3(4);
}

// ----------------------------------------
// peek_front

#[cfg(test)]
#[test]
fn test_peek_front() {
    let data1 = [1.0, 2.0, 3.0];
    let mut b = RingBuf::new(3);

    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);

    for i in 0..3 {
	let v = b.peek_front(i);
	for k in 0..i {
	    assert_eq!((k + 1) as f32, v.get(k));
	    assert_eq!((k + 1) as f32, v.get(k));
	}
    }
}

#[cfg(test)]
#[test]
fn test_cross_boundary_peek_front() {
    let data1 = [5.0, 1.0, 2.0, 3.0, 4.0];
    let mut data2 = [0.0; 4];
    let mut b = RingBuf::new(5);

    assert_eq!(2, b.read_from(&data1[0..2]));
    assert_partially_filled(&mut b, 2);

    assert_eq!(2, b.write_to(&mut data2[0..2]));
    assert_empty(& mut b);

    assert_eq!(4, b.read_from(&data1[1..5]));
    assert_eq!(1, b.read_from(&data1[0..1]));
    assert_full(&mut b);

    for i in 0..5 {
	let v = b.peek_front(i);
	for k in 0..i {
	    assert_eq!((k + 1) as f32, v.get(k));
	    assert_eq!((k + 1) as f32, v.get(k));
	}
    }
}

// ----------------------------------------
// Special checks

struct AS {
    b : RingBuf,
}

#[cfg(test)]
fn test_windowed_scoped_write_read_aux1(_dummy : &mut [f32]) {
}

#[cfg(test)]
fn test_windowed_scoped_write_read_aux2(o : &mut AS) {
    let len0;
    let len1;
    {
	let b0 = o.b.wrbuf(4096);
	len0 = b0.len();
	test_windowed_scoped_write_read_aux1(b0);
	if true {
	    let b1 = o.b.wrbuf(0);
	    len1 = b1.len();
	    test_windowed_scoped_write_read_aux1(b1);
	} else {
	    len1 = 999;
	}
    }
    assert_full(&mut o.b);
    assert_eq!(4096, len0);
    assert_eq!(0, len1);
}

#[cfg(test)]
#[test]
fn test_windowed_scoped_write_read() {
    let mut rb = AS {
	b : RingBuf::new(4096),
    };
    test_windowed_scoped_write_read_aux2(&mut rb);
}

#[cfg(test)]
#[test]
fn test_bad_size_after_full_read() {
    let mut b = RingBuf::new(4096);
    let _ = b.wrbuf(4096);
    assert_eq!(0, b.read_pos);
    assert_eq!(OUTPUT_BUFFER_IS_FULL, b.write_pos);
    b.drop_back(15).unwrap();
}
