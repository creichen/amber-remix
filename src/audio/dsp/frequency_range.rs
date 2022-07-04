/// Frequency range for FlexPCMWriter

pub type Freq = usize;

/// A FreqRange is a range position-to-frequency mappings,
/// open to the end.
pub struct FreqRange {
    frequencies : Vec<(usize, Freq)>,
}

impl FreqRange {
    pub fn new() -> FreqRange {
	return FreqRange { frequencies : Vec::new(), };
    }

    pub fn append(&mut self, pos : usize, freq : Freq) {
	if self.frequencies.len() > 0 {
	    let (last_pos, last_freq) = self.frequencies[self.frequencies.len() - 1];
	    // No need to change
	    if last_freq == freq {
		return;
	    }
	    if last_pos >= pos {
		panic!("Trying to append position {pos} that precedes current last position {last_pos}");
	    }
	}
	self.frequencies.push((pos, freq));
    }

    /// Frequency and remaining samples after the given position
    pub fn get(&self, pos : usize) -> (Freq, Option<usize>) {
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
    pub fn slide(&mut self, offset : usize) {
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


#[cfg(test)]
#[test]
fn freqrange_add_get() {
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
fn freqrange_slide() {
    let mut fr = FreqRange::new();
    fr.append(0, 100);
    fr.append(3, 101);
    fr.append(7, 102);
    fr.append(12, 103);

    assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
	       &fr.frequencies[..]);

    assert_eq!((100, Some(3)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(2));
    assert_eq!((101, Some(4)), fr.get(3));
    assert_eq!((101, Some(1)), fr.get(6));
    assert_eq!((102, Some(5)), fr.get(7));
    assert_eq!((102, Some(1)), fr.get(11));
    assert_eq!((103, None), fr.get(12));
    assert_eq!((103, None), fr.get(100));

    fr.slide(0);

    assert_eq!([(0, 100), (3, 101), (7, 102), (12, 103)],
	       &fr.frequencies[..]);

    assert_eq!((100, Some(3)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(2));
    assert_eq!((101, Some(4)), fr.get(3));
    assert_eq!((101, Some(1)), fr.get(6));
    assert_eq!((102, Some(5)), fr.get(7));
    assert_eq!((102, Some(1)), fr.get(11));
    assert_eq!((103, None), fr.get(12));
    assert_eq!((103, None), fr.get(100));

    fr.slide(1);

    assert_eq!([(0, 100), (2, 101), (6, 102), (11, 103)],
	       &fr.frequencies[..]);

    assert_eq!((100, Some(2)), fr.get(0));
    assert_eq!((100, Some(1)), fr.get(1));
    assert_eq!((101, Some(4)), fr.get(2));
    assert_eq!((101, Some(1)), fr.get(5));
    assert_eq!((102, Some(5)), fr.get(6));
    assert_eq!((102, Some(1)), fr.get(10));
    assert_eq!((103, None), fr.get(11));
    assert_eq!((103, None), fr.get(100));

    fr.slide(2);

    assert_eq!((101, Some(4)), fr.get(0));
    assert_eq!((101, Some(1)), fr.get(3));
    assert_eq!((102, Some(5)), fr.get(4));
    assert_eq!((102, Some(1)), fr.get(8));
    assert_eq!((103, None), fr.get(9));
    assert_eq!((103, None), fr.get(100));

    fr.slide(6);

    assert_eq!((102, Some(3)), fr.get(0));
    assert_eq!((102, Some(1)), fr.get(2));
    assert_eq!((103, None), fr.get(3));
    assert_eq!((103, None), fr.get(100));

    assert_eq!(2, fr.frequencies.len());

    fr.slide(10000);

    assert_eq!((103, None), fr.get(0));
    assert_eq!((103, None), fr.get(100));

    assert_eq!(1, fr.frequencies.len());
}
