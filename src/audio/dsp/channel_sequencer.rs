// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Sequences song iterator channels into raw PCM audio streams that can be written
/// to a WAV file or played as-is.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::{rc::Rc, cell::RefCell, sync::Arc};
use crate::audio::{Freq, iterator_sequencer::IteratorSequencer, iterator::ArcPoly, samplesource::SincSampleSource, pcmplay::MonoPCM};
use super::{writer::{RcSyncWriter, self, PCMSyncObserver, RcPCMWriter}, crossfade_linear::LinearCrossfade, vtracker::TrackerSensor, pcmsync, mock_pcmwriter};

// ================================================================================
// AudioStream

#[derive(Clone)]
pub struct AudioStream<T> {
    audio : Vec<f32>,       // Mono stream
    meta : Vec<(usize, T)>, // Sorted metainformation
}

impl<T> AudioStream<T> {
    fn new<U>() -> AudioStream<U> {
	return AudioStream {
	    audio : vec![],
	    meta : vec![],
	}
    }

    fn pos(&self) -> usize {
	self.audio.len()
    }
}

impl<T> From<AudioStream<T>> for MonoPCM {
    fn from(astream: AudioStream<T>) -> Self {
	MonoPCM::new(Arc::new(astream.audio))
    }
}

// ================================================================================
// sequencing helpers

struct AStreamPCMSyncObserver<T> {
    stream : AudioStream<T>,
    source : RcPCMWriter,
    done : bool,
}

impl<T> AStreamPCMSyncObserver<T> {
    fn new<U>() -> AStreamPCMSyncObserver<U> {
	AStreamPCMSyncObserver {
	    stream : AudioStream::<T>::new(),
	    source : mock_pcmwriter::mock_pw(),
	    done : false,
	}
    }

    // Returns whether we are done
    fn read(&mut self) -> bool {
	let mut buf = [0.0; 1];
	self.source.borrow_mut().write_pcm(&mut buf);
	self.stream.audio.extend_from_slice(&buf);
	return self.done;
    }
}

impl PCMSyncObserver for AStreamPCMSyncObserver<()> {
    fn observe_write(&mut self, _result : writer::SyncPCMResult, _written : &[f32]) {
    }

    fn observe_sync(&mut self, timeslice : writer::Timeslice) {
	println!("Tick {timeslice}");
	if timeslice == 500 {
	    self.done = true;
	}
    }
}

// ================================================================================
// sequencing

/// Sequences an audioiterator into its constitutent (mono) streams using a sinc pipeline with
/// optional linear cross-fade
pub fn sequence_sinc_linear(polyit : ArcPoly, freq : Freq, linear_crossfade : usize) -> Vec<AudioStream<()>> {
    let (samples, arcits) = {
	let mut guard = polyit.lock().unwrap();
	(guard.get_samples(), guard.get())
    };
    let samplesource = Rc::new(RefCell::new(SincSampleSource::from_i8(freq, samples.as_ref())));
    let sync = pcmsync::new_basic();

    let mut observers = vec![];

    for arcit in arcits {
	let itseq_base = Rc::new(RefCell::new(IteratorSequencer::new_with_source(arcit, freq, 1, samplesource.clone(), TrackerSensor::new())));
	let itseq : RcSyncWriter = if linear_crossfade == 0 { itseq_base.clone() } else {
	    LinearCrossfade::new_rc(linear_crossfade, itseq_base.clone())
	};
	let observer = Rc::new(RefCell::new(AStreamPCMSyncObserver::<()>::new()));
	let itseq = writer::observe_rc_sync(itseq, observer.clone());
	let itseq = sync.borrow_mut().sync(itseq.clone());
	observer.borrow_mut().source = itseq;
	observers.push(observer);
    }

    let mut done = false;
    while !done {
	done = true;
	for observer in observers.iter_mut() {
	    done &= observer.borrow_mut().read();
	}
    };

    return observers.into_iter().map(|obs| obs.borrow().stream.clone()).collect();
}
