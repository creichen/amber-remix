use std::{fmt::Display, cell::RefCell, rc::Rc};

/// Frequency range for FlexPCMWriter

pub type Freq = usize;

/// A FreqRange is a range position-to-frequency mappings,
/// open to the end.
#[derive(Clone)]
pub enum FreqRange {
    Base(Rc<RefCell<FreqRangeBase>>),
    AtOffset(Rc<RefCell<FreqRangeBase>>, usize),
}

pub struct FreqRangeBase {
    frequencies : Vec<(usize, Freq)>,
}

impl FreqRangeBase {
    fn new() -> FreqRangeBase {
	return FreqRangeBase { frequencies : Vec::new(), };
    }

    fn append(&mut self, pos : usize, freq : Freq) {
	if self.frequencies.len() > 0 {
	    let (last_pos, last_freq) = self.frequencies[self.frequencies.len() - 1];
	    // No need to change
	    if last_freq == freq {
		return;
	    }
	    // allow update
	    if last_pos == pos {
		self.frequencies.pop();
	    } else if last_pos > pos {
		panic!("Trying to append position {pos} that precedes current last position {last_pos}");
	    }
	}
	self.frequencies.push((pos, freq));
    }

    pub fn is_empty(&self) -> bool {
	return self.frequencies.len() == 0;
    }

    fn unbounded_after(&self, offset : usize) -> bool {
	if let (_, None) = self.get(offset) { true } else { false }
    }

    /// Frequency and remaining samples after the given position
    fn get(&self, pos : usize) -> (Freq, Option<usize>) {
	let mut last_pos = None;
	for rev_i in 0..self.frequencies.len() {
	    let (i_pos, i_freq) = self.frequencies[self.frequencies.len() - rev_i - 1];
	    if i_pos <= pos {
		let duration = match last_pos {
		    None     => None,
		    Some(lp) => Some(lp - pos),
		};
		return (i_freq, duration);
	    } else {
		last_pos = Some(i_pos);
	    }
	}
	panic!("No frequencies known in range");
    }

    /// Slide window to the left, discarding old data
    fn slide(&mut self, offset : usize) {
	let mut new_frequencies = vec![];
	let freq = &self.frequencies;
	let mut last_freq = None;
	for (i_pos, i_freq) in freq {
	    if *i_pos < offset {
		last_freq = Some(*i_freq);
	    } else if *i_pos == offset {
		last_freq = None;
	    }

	    if *i_pos >= offset {
		match last_freq {
		    Some (freq) => { new_frequencies.push((0, freq)); }
		    None        => {}
		}
		new_frequencies.push((*i_pos - offset, *i_freq));
		last_freq = None;
	    }
	}
	match last_freq {
	    Some (freq) => { new_frequencies.push((0, freq)); }
	    None        => {}
	}
	self.frequencies = new_frequencies;
    }
}

impl Display for FreqRangeBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	write!(f, "{:?}", self.frequencies)
    }
}

impl<'a> Display for FreqRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	match self {
	    FreqRange::Base(b)        => write!(f, "B({})", b.borrow()),
	    FreqRange::AtOffset(b, o) => write!(f, "O({} + {o})", b.borrow()),
	}
    }
}

impl FreqRange {
    pub fn new() -> FreqRange {
	return FreqRange::Base(Rc::new(RefCell::new(FreqRangeBase::new())));
    }

    pub fn append(&mut self, pos : usize, freq : Freq) {
	match self {
	    FreqRange::Base(f)                => f.borrow_mut().append(pos, freq),
	    FreqRange::AtOffset(bref, offset) => { bref.borrow_mut().append(pos + *offset, freq)},
	}
    }

    pub fn is_empty(&self) -> bool {
	match self {
	    FreqRange::Base(f)                => f.borrow().is_empty(),
	    FreqRange::AtOffset(bref, offset) => bref.borrow().is_empty() || bref.borrow().unbounded_after(*offset),
	}
    }

    /// Frequency and remaining samples after the given position
    pub fn get(&self, pos : usize) -> (Freq, Option<usize>) {
	match self {
	    FreqRange::Base(f)                => f.borrow().get(pos),
	    FreqRange::AtOffset(bref, offset) => bref.borrow().get(pos + offset),
	}
    }

    /// Slide window to the left, discarding old data
    pub fn shift(&mut self, offset : usize) {
	match self {
	    FreqRange::Base(f)           => f.borrow_mut().slide(offset),
	    FreqRange::AtOffset(bref, _) => bref.borrow_mut().slide(offset),
	}
    }

    pub fn at_offset(&mut self, offset : usize) -> FreqRange {
	match self {
	    FreqRange::Base(f)           => FreqRange::AtOffset(f.clone(), offset),
	    FreqRange::AtOffset(bref, o) => FreqRange::AtOffset(bref.clone(), offset + *o),
	}
    }
}


#[cfg(test)]
#[test]
fn test_append_get() {
    let mut fr = FreqRange::new();
    fr.append(0, 42);
    assert_eq!((42, None), fr.get(0));
    assert_eq!((42, None), fr.get(10));
    assert_eq!((42, None), fr.get(1000));

    fr.append(10, 80);

    assert_eq!((42, Some(10)), fr.get(0));
    assert_eq!((42, Some(1)), fr.get(9));
    assert_eq!((80, None), fr.get(10));
    assert_eq!((80, None), fr.get(1000));
}

#[cfg(test)]
#[test]
fn test_slide() {
    let mut fr = FreqRange::new();
    fr.append(0, 100);
    fr.append(3, 101);
    fr.append(7, 102);
    fr.append(12, 103);

    if let FreqRange::Base(ref frr) = fr {
	assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
		   &frr.borrow().frequencies[..]);
    } else { panic!(); }

    assert_eq!((100, Some(3)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(2));
    assert_eq!((101, Some(4)), fr.get(3));
    assert_eq!((101, Some(1)), fr.get(6));
    assert_eq!((102, Some(5)), fr.get(7));
    assert_eq!((102, Some(1)), fr.get(11));
    assert_eq!((103, None), fr.get(12));
    assert_eq!((103, None), fr.get(100));

    fr.shift(0);

    if let FreqRange::Base(ref frr) = fr {
	assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
		   &frr.borrow().frequencies[..]);
    } else { panic!(); }

    assert_eq!((100, Some(3)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(2));
    assert_eq!((101, Some(4)), fr.get(3));
    assert_eq!((101, Some(1)), fr.get(6));
    assert_eq!((102, Some(5)), fr.get(7));
    assert_eq!((102, Some(1)), fr.get(11));
    assert_eq!((103, None), fr.get(12));
    assert_eq!((103, None), fr.get(100));

    fr.shift(1);

    if let FreqRange::Base(ref frr) = fr {
	assert_eq!([(0, 100), (2, 101), (6, 102), (11, 103)],
		   &frr.borrow().frequencies[..]);
    } else { panic!(); }

    assert_eq!((100, Some(2)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(1));
    assert_eq!((101, Some(4)), fr.get(2));
    assert_eq!((101, Some(1)), fr.get(5));
    assert_eq!((102, Some(5)), fr.get(6));
    assert_eq!((102, Some(1)), fr.get(10));
    assert_eq!((103, None), fr.get(11));
    assert_eq!((103, None), fr.get(100));

    fr.shift(2);

    assert_eq!((101, Some(4)), fr.get(0));
    assert_eq!((101, Some(1)), fr.get(3));
    assert_eq!((102, Some(5)), fr.get(4));
    assert_eq!((102, Some(1)), fr.get(8));
    assert_eq!((103, None), fr.get(9));
    assert_eq!((103, None), fr.get(100));

    fr.shift(6);

    assert_eq!((102, Some(3)), fr.get(0));
    assert_eq!((102, Some(1)), fr.get(2));
    assert_eq!((103, None), fr.get(3));
    assert_eq!((103, None), fr.get(100));

    if let FreqRange::Base(ref frr) = fr {
	assert_eq!(2, frr.borrow().frequencies.len());
    } else { panic!(); }

    fr.shift(10000);

    assert_eq!((103, None), fr.get(0));
    assert_eq!((103, None), fr.get(100));

    if let FreqRange::Base(ref frr) = fr {
	assert_eq!(1, frr.borrow().frequencies.len());
    } else { panic!(); }
}

#[cfg(test)]
#[test]
fn test_at_offset_get() {
    let mut fr_base = FreqRange::new();
    fr_base.append(0, 100);
    fr_base.append(3, 101);
    fr_base.append(7, 102);
    fr_base.append(12, 103);

    let fr = fr_base.at_offset(0);

    assert_eq!((100, Some(3)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(2));
    assert_eq!((101, Some(4)), fr.get(3));
    assert_eq!((101, Some(1)), fr.get(6));
    assert_eq!((102, Some(5)), fr.get(7));
    assert_eq!((102, Some(1)), fr.get(11));
    assert_eq!((103, None), fr.get(12));
    assert_eq!((103, None), fr.get(100));

    let mut fr1 = fr_base.at_offset(1);

    assert_eq!((100, Some(2)), fr1.get(0));
    assert_eq!((100, Some(1)), fr1.get(1));
    assert_eq!((101, Some(4)), fr1.get(2));
    assert_eq!((101, Some(1)), fr1.get(5));
    assert_eq!((102, Some(5)), fr1.get(6));
    assert_eq!((102, Some(1)), fr1.get(10));
    assert_eq!((103, None), fr1.get(11));
    assert_eq!((103, None), fr1.get(100));

    let fr2 = fr1.at_offset(2);

    assert_eq!((101, Some(4)), fr2.get(0));
    assert_eq!((101, Some(1)), fr2.get(3));
    assert_eq!((102, Some(5)), fr2.get(4));
    assert_eq!((102, Some(1)), fr2.get(8));
    assert_eq!((103, None), fr2.get(9));
    assert_eq!((103, None), fr2.get(100));
}

#[cfg(test)]
#[test]
fn test_at_offset_append() {
    let expected = [(100, Some(3)), // 0
		    (100, Some(2)),
		    (100, Some(1)),
		    (101, Some(1)),
		    (102, Some(1)),
		    (103, Some(2)), // 5
		    (103, Some(1)),
		    (104, None),
		    (104, None), ];

    let mut fr_base = FreqRange::new();
    fr_base.append(0, 100);
    fr_base.append(3, 101);

    {
	let mut fr = fr_base.at_offset(0);
	fr.append(4, 102);
    }

    {
	let mut fr1 = fr_base.at_offset(5);

	fr1.append(0, 103); // 5

	{
	    let mut fr2 = fr1.at_offset(1);
	    fr2.append(1, 104); // 7

	    for (i, expected) in expected.iter().enumerate() {
		if i >= 6 {
		    assert_eq!(*expected, fr2.get(i - 6));
		}
	    }
	}
	for (i, expected) in expected.iter().enumerate() {
	    if i >= 5 {
		assert_eq!(*expected, fr1.get(i - 5));
	    }
	}
    }

    for (i, expected) in expected.iter().enumerate() {
	assert_eq!(*expected, fr_base.get(i));
    }
    let fr = fr_base.at_offset(0);
    for (i, expected) in expected.iter().enumerate() {
	assert_eq!(*expected, fr.get(i));
    }
}

#[cfg(test)]
fn require_base(r : &FreqRange) -> &Rc<RefCell<FreqRangeBase>> {
    match r {
	FreqRange::Base(b) => b,
	_ => panic!(),
    }
}

#[cfg(test)]
#[test]
fn test_at_offset_slide() {
    let mut fr_base = FreqRange::new();
    fr_base.append(0, 100);
    fr_base.append(3, 101);
    fr_base.append(7, 102);
    fr_base.append(12, 103);

    let frr = require_base(&fr_base);
    assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
	       &frr.borrow().frequencies[..]);

    assert_eq!((100, Some(3)), fr_base.get(0));
    assert_eq!((100, Some(1)), fr_base.get(2));
    assert_eq!((101, Some(4)), fr_base.get(3));
    assert_eq!((101, Some(1)), fr_base.get(6));
    assert_eq!((102, Some(5)), fr_base.get(7));
    assert_eq!((102, Some(1)), fr_base.get(11));
    assert_eq!((103, None), fr_base.get(12));
    assert_eq!((103, None), fr_base.get(100));

    {
	let mut fr = fr_base.at_offset(0);
	fr.shift(0);
    }

    {
	let frr = require_base(&fr_base);
	assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
		   &frr.borrow().frequencies[..]);
    }

    {
	let mut fr = fr_base.at_offset(0);

	assert_eq!((100, Some(3)), fr.get(0));
	assert_eq!((100, Some(1)), fr.get(2));
	assert_eq!((101, Some(4)), fr.get(3));
	assert_eq!((101, Some(1)), fr.get(6));
	assert_eq!((102, Some(5)), fr.get(7));
	assert_eq!((102, Some(1)), fr.get(11));
	assert_eq!((103, None), fr.get(12));
	assert_eq!((103, None), fr.get(100));

	fr.shift(1);
    }

    {
	let frr = require_base(&fr_base);
	assert_eq!([(0, 100), (2, 101), (6, 102), (11, 103)],
		   &frr.borrow().frequencies[..]);
    }

    assert_eq!((100, Some(2)), fr_base.get(0));
    assert_eq!((100, Some(1)), fr_base.get(1));
    assert_eq!((101, Some(4)), fr_base.get(2));
    assert_eq!((101, Some(1)), fr_base.get(5));
    assert_eq!((102, Some(5)), fr_base.get(6));
    assert_eq!((102, Some(1)), fr_base.get(10));
    assert_eq!((103, None), fr_base.get(11));
    assert_eq!((103, None), fr_base.get(100));

    {
	let mut fr2 = fr_base.at_offset(1);
	let mut fr3 = fr2.at_offset(3);
	fr3.shift(2);
    }

    assert_eq!((101, Some(4)), fr_base.get(0));
    assert_eq!((101, Some(1)), fr_base.get(3));
    assert_eq!((102, Some(5)), fr_base.get(4));
    assert_eq!((102, Some(1)), fr_base.get(8));
    assert_eq!((103, None), fr_base.get(9));
    assert_eq!((103, None), fr_base.get(100));

}
