#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

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

    pub fn capacity(&self) -> usize {
	return self.data.len();
    }

    // Shrink buffer contents to size 0
    pub fn reset(&self) {
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
	    return to_write;
	}
	// Otherwise, we must have hit the end of the buffer
	// Call ourselves one final time to finish up
	self.read_pos -= self.capacity();
	return to_write + self._write_to(&mut dest[to_write..]);
    }

    /// Remove the specified number of most recently added samples
    /// Return how many were actually removed
    pub fn unread(&mut self, to_remove : usize) -> Result<usize, String> {
	if to_remove > self.len() {
	    return Err(format!("Insufficient capacity: requested unread({to_remove}) on only {} elements", self.len()));
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
	// println!("{} {}", self.write_pos, self.capacity());
	// self.write_pos -= self.capacity();
	return to_write + self.read_from(&src[to_write..]);
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
	println!("{start_pos}..{end_pos} {avail} {size} {} {}", self.write_pos, self.read_pos);
	self._advance_write_pos(end_pos - start_pos);
	return &mut self.data[start_pos..end_pos];
    }
}


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
    let mut data2 = [0.0; 3];
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
    let mut data2 = [0.0; 3];
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

    assert_eq!(1, b.read_from(&data1[0..3]));
    assert_eq!(1, b.write_to(&mut data2[0..3]));
    assert_eq!(3, b.read_from(&data1[0..3]));
    assert_full(&mut b);
    b.reset();
    assert_empty(&mut b);
}
