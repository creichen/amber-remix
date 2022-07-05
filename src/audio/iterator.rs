use std::{collections::VecDeque, sync::{Arc, Mutex}};

use super::{dsp::frequency_range::Freq, SampleRange};


/**
 * Audio queue operations allow AudioIterators to control output to their channel.
 *
 * "X ; WaitMillis(n); Y" means that settings X will be in effect for "n" milliseconds,
 * then any changes from Y take effect.
 */
#[derive(Clone, Debug)]
pub enum AQOp {
    /// Process channel settings for specified nr of milliseconds
    WaitMillis(usize),
    /// Enqueue to the sample queue (applies after the current sample finishes playing)
    SetSamples(Vec<AQSample>),
    /// Set audio frequency in Hz (applies at the start of the next sample)
    SetFreq(Freq),
    /// Set audio volume as fraction (applies immediately)
    SetVolume(f32),
}

#[derive(Clone, Copy, Debug)]
pub enum AQSample {
    /// Loop specified sample
    Loop(SampleRange),
    /// Play specified sample once
    Once(SampleRange),
}

pub type ArcIt = Arc<Mutex<dyn AudioIterator>>;

pub trait AudioIterator : Send + Sync {
    fn next(&mut self, queue : &mut VecDeque<AQOp>);
}
// ----------------------------------------

pub fn mock(v : Vec<Vec<AQOp>>) -> ArcIt {
    return Arc::new(Mutex::new(MockAudioIterator::new(v)));
}

pub fn simple(v : Vec<AQOp>) -> ArcIt {
    return mock(vec![v]);
}

pub fn silent() -> ArcIt {
    return simple(vec![AQOp::SetFreq(1000), AQOp::WaitMillis(1000)]);
}

/// MockAudioIterator For testing
pub struct MockAudioIterator {
    pub ops : VecDeque<Vec<AQOp>>,
}

impl MockAudioIterator {
    pub fn new(ops : Vec<Vec<AQOp>>) -> MockAudioIterator {
	MockAudioIterator {
	    ops : VecDeque::from(ops),
	}
    }

    fn len(&self) -> usize {
	return self.ops.len();
    }

    fn num_elements(&self) -> usize {
	let mut n = 0;
	for v in &self.ops {
	    n += v.len();
	}
	return n;
    }

    fn get<'a>(&'a self, i : usize) -> &'a[AQOp] {
	let v = &self.ops[i];
	return &v[..];
    }
}

impl AudioIterator for MockAudioIterator {
    fn next(&mut self, queue : &mut VecDeque<AQOp>) {
	match self.ops.pop_front() {
	    None     => {},
	    Some(vv) => { queue.append(&mut VecDeque::from(vv)); },
	}
    }
}