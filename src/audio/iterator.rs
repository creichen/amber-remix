// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::{collections::VecDeque, sync::{Arc, Mutex}};
use crate::datafiles::music::BasicSample;
use super::{Timeslice, Freq, SampleRange, streamlog::{StreamLogClient, ArcStreamLogger}};

// ================================================================================
// AudioIterator

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
    /// Timeslice transition marker; ensures synchronisation for minor variations in how
    /// WaitMillis is interpreted (if sent after WaitMillis)
    Timeslice(Timeslice),
    /// Enqueue to the sample queue (applies after the current sample finishes playing)
    SetSamples(Vec<AQSample>),
    /// Set audio frequency in Hz (applies at the start of the next sample)
    SetFreq(Freq),
    /// Set audio volume as fraction (applies immediately)
    SetVolume(f32),
    End,
}

#[derive(Clone, Copy, Debug)]
pub enum AQSample {
    /// Loop specified sample
    Loop(SampleRange),
    /// Play specified sample once
    Once(SampleRange),
    /// Play specified sample once, but carry over the previous sample's offset.
    /// The optional value is filled in by the audio queue processor.
    /// This is useful for "slider" samples that have closely aligned waveforms and switch out frequently.
    OnceAtOffset(SampleRange, Option<(usize, usize)>),
}

impl std::fmt::Display for AQSample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AQSample::Loop(sr)              => write!(f, "Loop({sr})"),
            AQSample::Once(sr)              => write!(f, "Once({sr})"),
            AQSample::OnceAtOffset(sr, off) => write!(f, "OnceAtOffset({sr}, {off:?})"),
        }
    }
}

impl From<BasicSample> for AQOp {
    fn from(bs: BasicSample) -> Self {
	let att = AQSample::Once(bs.attack);
	match bs.looping {
	    None    => AQOp::SetSamples(vec![att]),
	    Some(l) => AQOp::SetSamples(vec![att, AQSample::Loop(l)]),
	}
    }
}

pub type ArcIt = Arc<Mutex<dyn AudioIterator>>;

pub trait AudioIterator : Send + Sync + StreamLogClient {
    fn next(&mut self, queue : &mut VecDeque<AQOp>);

    /// Duplicates the song in its current state
    fn clone_it(&self) -> ArcIt;
}

// ----------------------------------------

pub fn mock(v : Vec<Vec<AQOp>>) -> ArcIt {
    return Arc::new(Mutex::new(MockAudioIterator::new(v)));
}

pub fn simple(v : Vec<AQOp>) -> ArcIt {
    return mock(vec![v]);
}

// pub fn silent() -> ArcIt {
//     return simple(vec![
// 	AQOp::SetVolume(0.0), AQOp::SetFreq(1000), AQOp::WaitMillis(20), AQOp::Timeslice(1),
// //	AQOp::WaitMillis(20), AQOp::Timeslice(2),
//     ]);
// }

pub fn empty() -> ArcIt {
    return simple(vec![
	AQOp::End,
    ]);
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

    #[allow(unused)]
    fn len(&self) -> usize {
	return self.ops.len();
    }

    #[allow(unused)]
    fn num_elements(&self) -> usize {
	let mut n = 0;
	for v in &self.ops {
	    n += v.len();
	}
	return n;
    }

    #[allow(unused)]
    fn get<'a>(&'a self, i : usize) -> &'a[AQOp] {
	let v = &self.ops[i];
	return &v[..];
    }
}

impl StreamLogClient for MockAudioIterator {
    fn set_logger(&mut self, _logger : super::streamlog::ArcStreamLogger) {
    }
}

impl AudioIterator for MockAudioIterator {
    fn next(&mut self, queue : &mut VecDeque<AQOp>) {
	match self.ops.pop_front() {
	    None     => {},
	    Some(vv) => { queue.append(&mut VecDeque::from((&vv[..]).to_vec()));
			  //self.ops.push_back((&vv[..]).to_vec());
	    },
	}
    }

    fn clone_it(&self) -> ArcIt {
	let mut ops = VecDeque::new();
	for op in &self.ops {
	    ops.push_back((op[..]).to_vec());
	}
	return Arc::new(Mutex::new(MockAudioIterator {
	    ops,
	}));
    }
}

// ================================================================================
// PolyIterator

pub type ArcPoly = Arc<Mutex<dyn PolyIterator>>;

pub trait PolyIterator : Send + Sync {
    /// Retrieves audio iterators for all channels
    /// Currently assumes four channels with Amiga stereo bindings (LRRL)
    fn get(&mut self) -> Vec<ArcIt>;

    /// This is a no-op if the sapmles for this song are included in the song itself
    fn set_default_samples(&mut self, samples : Arc<Vec<i8>>);

    /// Retrieves the audio samples that are indexed by AQSample
    fn get_samples(&self) -> Arc<Vec<i8>>;
}

// ================================================================================
// Observers

// ----------------------------------------
// AudioIteratorObserver

pub trait AudioIteratorObserver : Send + Sync {
    fn observe_aqop(&mut self, result : &AQOp);
}

type ArcItObserver = Arc<Mutex<dyn AudioIteratorObserver>>;

#[derive(Clone)]
struct AudioIteratorObserverAdapter {
    pub source : ArcIt,
    observer : ArcItObserver,
}

pub fn observe(source : ArcIt, observer : ArcItObserver) -> ArcIt {
    return Arc::new(Mutex::new(AudioIteratorObserverAdapter {
	source,
	observer : observer.clone(),
    }));
}

impl StreamLogClient for AudioIteratorObserverAdapter {
    fn set_logger(&mut self, logger : ArcStreamLogger) {
	let mut guard = self.source.lock().unwrap();
	guard.set_logger(logger);
    }
}

impl AudioIterator for AudioIteratorObserverAdapter {
    fn next(&mut self, queue : &mut VecDeque<AQOp>) {
	let mut tmpqueue = {
	    let mut q = VecDeque::new();
	    let mut guard = self.source.lock().unwrap();
	    guard.next(&mut q);
	    q
	};
	for e in tmpqueue.drain(..) {
	    let mut guard = self.observer.lock().unwrap();
	    guard.observe_aqop(&e);
	    queue.push_back(e);
	}
    }

    fn clone_it(&self) -> ArcIt {
	panic!("Obsever cloning would probably lead to unexpected results, therefore unimplemented for now");
    }
}
