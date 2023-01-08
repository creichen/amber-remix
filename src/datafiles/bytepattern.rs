// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::fmt::Write;

#[derive(Clone)]
pub struct BPInfer {
    observed : [u128; 2], // bit mask
}

const EMPTY : BPInfer = BPInfer {
    observed : [0, 0],
};

impl BPInfer {

    #[allow(unused)]
    pub fn new() -> BPInfer {
	EMPTY
    }

    pub fn new_vec(size : usize) -> Vec<BPInfer> {
	std::iter::repeat(EMPTY).take(size).collect::<Vec<_>>()
    }

    pub fn new_2dvec(outer_size : usize, inner_size : usize) -> Vec<Vec<BPInfer>> {
	let mut result = vec![];
	for _i in 0..outer_size {
	    result.push(BPInfer::new_vec(inner_size));
	}
	return result;
    }

    pub fn observe(&mut self, byte : u8) {
	self.observed[(byte >> 7) as usize] |= 1 << (byte & 0x7f);
    }

    pub fn observe_vec(vec : &mut Vec<BPInfer>, data : &[u8]) {
	for (i, bpi) in vec.iter_mut().enumerate() {
	    bpi.observe(data[i]);
	}
    }

    pub fn explain_as_individuals(&self) -> Vec<u8> {
	let mut result = vec![];

	for x in 0..128 {
	    if (self.observed[0] & 1 << x) > 0 {
		result.push(x);
	    }
	}
	for x in 0..128 {
	    if (self.observed[1] & 1 << x) > 0 {
		result.push(x + 128);
	    }
	}

	return result;
    }

    pub fn explain_as_ranges(&self) -> Vec<(u8, u8)> {
	let mut result = vec![];
	let mut start = 0;
	let mut last = None;
	for n in self.explain_as_individuals() {
	    if last == None {
		start = n
	    } else if last != Some(n - 1) {
		match last {
		    None    => {},
		    Some(v) => {
			result.push((start, v));
			start = n;
		    }
		}
	    }
	    last = Some(n);
	}
	// handle the final range:
	match last {
	    None   => {}
	    Some(v) => {
		result.push((start, v));
	    }
	}

	return result;
    }

    /// return (bitmask, #max_bits_simultaneously, mutually_exclusive_flags)
    pub fn explain_as_flags(&self) -> (u8, usize, Vec<u8>) {
	let mut max_bits = 0;
	let mut all_bits = 0;
	let mut mutual_occurrences = [0, 0, 0, 0, 0, 0, 0, 0]; // which flags may each bit co-occur with?
	for byte in self.explain_as_individuals() {
	    all_bits |= byte;
	    for i in 0..8 {
		if 0 != byte & 1 << i {
		    mutual_occurrences[i] |= byte;
		}
	    }
	    max_bits = u32::max(max_bits, byte.count_ones());
	}

	let mut mutual_exclusions_vec = vec![];

	for i in 0..8 {
	    if mutual_occurrences[i] != 0 {
		// did we ever observe this flag?

		// Set only bits that we never co-occur with
		let mut mutual_exclusions : u8 = (1 << i) | ! mutual_occurrences[i];
		//pwarn!("mex[{i}] = {:x}", mutual_exclusions);

		for k in 0..8 {
		    // Only keep what all proposed mutually exclusive objects agree with
		    if 0 != (all_bits & (1 << k)) // does the flag occur at all?
			&& 0 != (mutual_exclusions & (1 << k)) {
			    //pwarn!("  refine {k} &= {:x}", ((1 << k) | ! mutual_occurrences[k]));
			    mutual_exclusions &= (1 << k) | ! mutual_occurrences[k];
			} else {
			    // strip out this flag
			    mutual_exclusions &= !(1 << k);
			}
		    //pwarn!("  -{k}-> {:x}", mutual_exclusions);
		}

		//pwarn!("  ==> {:x}", mutual_exclusions);

		// if we have a unique mutual exclusions set, record it
		if mutual_exclusions.count_ones() > 1 {
		    let mut found = false;
		    for m in &mutual_exclusions_vec {
			if *m == mutual_exclusions {
			    found = true;
			    break;
			}
		    }
		    if !found {
			mutual_exclusions_vec.push(mutual_exclusions);
		    }
		}
	    }
	}

	return (all_bits, max_bits as usize, mutual_exclusions_vec);
    }

    pub fn explain_individuals(&self) -> String {
	let mut result = "[".to_string();
	let mut first = true;
	for byte in self.explain_as_individuals() {
	    if first {
		first = false;
	    } else {
		result += ",";
	    }
	    write!(result, "{:02x}", byte).unwrap();
	}
	result += "]";
	return result;
    }

    pub fn explain_ranges(&self) -> String {
	let mut result = "[".to_string();
	let mut first = true;
	for (l, r) in self.explain_as_ranges() {
	    if first {
		first = false;
	    } else {
		result += ",";
	    }
	    if l == r {
		write!(result, "{:02x}", l).unwrap();
	    } else {
		write!(result, "{:02x}-{:02x}", l, r).unwrap();
	    }
	}
	result += "]";
	return result;
    }

    pub fn explain_flags(&self) -> String {
	let mut result = "{mask:".to_string();

	let (flags, _max_bits, mutex_bits) = self.explain_as_flags();
	write!(result, "{:02x}", flags).unwrap();

	if mutex_bits.len() > 0 {
	    let mut first = true;
	    result += "/exclusive:";
	    for mb in mutex_bits {
		if first {
		    first = false;
		} else {
		    result += ",";
		}
		write!(result, "{:02x}", mb).unwrap();
	    }
	}
	result += "}";
	return result;
    }

    pub fn explain(&self) -> String {
	let individuals = self.explain_as_individuals();
	let ranges = self.explain_as_ranges();
	let (flags, max_bits, mutex_bits) = self.explain_as_flags();

	let cost_individuals = (individuals.len() as i32) * 3;
	let cost_ranges = (ranges.len() as i32) * 2;
	let cost_flags = (flags.count_ones() as i32) + (max_bits as i32 * 2) - (mutex_bits.len() as i32);

	const INDIVIDUAL: u8 = 1;
	const RANGE	: u8 = 2;
	const FLAGS	: u8 = 3;

	let (mut best, cost) = if cost_individuals < cost_ranges {
	    (INDIVIDUAL, cost_individuals)
	} else {
	    (RANGE, cost_ranges)
	};

	if cost_flags < cost {
	    best = FLAGS;
	}

	if individuals.len() < 2 {
	    best = INDIVIDUAL;
	}

	match best {
	    FLAGS => self.explain_flags(),
	    RANGE => self.explain_ranges(),
	    _     => self.explain_individuals(),
	}
    }

    pub fn explain_vec(vec : &Vec<BPInfer>) -> String {
	vec.iter().map(|pbi| pbi.explain()).collect::<Vec<String>>().join("  ")
    }
}


#[cfg(test)]
#[test]
fn test_zero() {
    let b0 = BPInfer::new();
    assert_eq!(Vec::<u8>::new(),
	       b0.explain_as_individuals());
    assert_eq!(Vec::<(u8, u8)>::new(),
	       b0.explain_as_ranges());
    assert_eq!((0, 0, Vec::<u8>::new()),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_one() {
    let mut b0 = BPInfer::new();
    b0.observe(7);
    assert_eq!(vec![7],
	       b0.explain_as_individuals());
    assert_eq!(vec![(7,7)],
	       b0.explain_as_ranges());
    assert_eq!((7, 3, Vec::<u8>::new()),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_two_adjacent() {
    let mut b0 = BPInfer::new();
    b0.observe(8);
    b0.observe(7);
    assert_eq!(vec![7, 8],
	       b0.explain_as_individuals());
    assert_eq!(vec![(7, 8)],
	       b0.explain_as_ranges());
    assert_eq!((0xf, 3, vec![0x09, 0x0a, 0x0c]),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_two_split() {
    let mut b0 = BPInfer::new();
    b0.observe(4);
    b0.observe(2);
    assert_eq!(vec![2, 4],
	       b0.explain_as_individuals());
    assert_eq!(vec![(2, 2), (4, 4)],
	       b0.explain_as_ranges());
    assert_eq!((0x06, 1, vec![0x06]),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_range() {
    let mut b0 = BPInfer::new();
    for i in 4..8 {
	b0.observe(i);
    }
    for i in 5..9 {
	b0.observe(i);
    }
    assert_eq!(vec![4, 5, 6, 7, 8],
	       b0.explain_as_individuals());
    assert_eq!(vec![(4, 8)],
	       b0.explain_as_ranges());
    assert_eq!((0x0f, 3, vec![0x09, 0x0a, 0x0c]),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_range_high() {
    let mut b0 = BPInfer::new();
    b0.observe(0x82);
    b0.observe(0x84);
    b0.observe(0x83);
    assert_eq!(vec![0x82, 0x83, 0x84],
	       b0.explain_as_individuals());
    assert_eq!(vec![(0x82, 0x84)],
	       b0.explain_as_ranges());
    assert_eq!((0x87, 3, vec![0x05, 0x06]),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_bounds() {
    let mut b0 = BPInfer::new();
    b0.observe(0xfd);
    b0.observe(0xfe);
    b0.observe(0xff);
    assert_eq!(vec![0xfd, 0xfe, 0xff],
	       b0.explain_as_individuals());
    assert_eq!(vec![(0xfd, 0xff)],
	       b0.explain_as_ranges());
    assert_eq!((0xff, 8, vec![]),
	       b0.explain_as_flags());
}

#[cfg(test)]
#[test]
fn test_flags() {
    let mut b0 = BPInfer::new();
    b0.observe(0x00);
    b0.observe(0x12);
    b0.observe(0x02);
    b0.observe(0x10);
    b0.observe(0x20);
    b0.observe(0x00);
    b0.observe(0x12);
    b0.observe(0x30);
    assert_eq!(vec![0x00, 0x02, 0x10, 0x12, 0x20, 0x30],
	       b0.explain_as_individuals());
    assert_eq!(vec![
	(0x00, 0x00),
	(0x02, 0x02),
	(0x10, 0x10),
	(0x12, 0x12),
	(0x20, 0x20),
	(0x30, 0x30), ],
	       b0.explain_as_ranges());
    assert_eq!((0x32, 2, vec![0x22]),
	       b0.explain_as_flags());

}

#[cfg(test)]
#[test]
fn test_all() {
    let mut b0 = BPInfer::new();
    for i in 0..=255 {
	b0.observe(i);
    }
    assert_eq!(256,
	       b0.explain_as_individuals().len());
    assert_eq!(vec![(0, 255)],
	       b0.explain_as_ranges());
    assert_eq!((0xff, 8, vec![]),
	       b0.explain_as_flags());
}
