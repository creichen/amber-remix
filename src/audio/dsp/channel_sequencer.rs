// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Sequences song iterator channels into raw PCM audio streams that can be written
/// to a WAV file or played as-is.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use atomic_option::AtomicOption;
use std::{rc::Rc, cell::RefCell};
use crate::audio::{Freq, iterator_sequencer::IteratorSequencer, iterator::ArcPoly, samplesource::SincSampleSource};
use super::{writer::{RcSyncWriter, self, PCMSyncObserver}, crossfade_linear::LinearCrossfade, vtracker::TrackerSensor};

struct AudioStream<T> {
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

struct AStreamPCMSyncObserver<T> {
    stream : AtomicOption<AudioStream<T>>,
}

impl<T> AStreamPCMSyncObserver<T> {
    fn new<U>() -> AStreamPCMSyncObserver<U> {
	AStreamPCMSyncObserver {
	    stream : AtomicOption::new(Box::new(AudioStream::<T>::new())),
	}
    }
}

impl PCMSyncObserver for AStreamPCMSyncObserver<()> {
    fn observe_write(&mut self, result : writer::SyncPCMResult, written : &[f32]) {
    }

    fn observe_sync(&mut self, timeslice : writer::Timeslice) {
	println!("Tick {timeslice}");
    }
}

/// Sequences an audioiterator into its constitutent (mono) streams using a sinc pipeline with
/// optional linear cross-fade
pub fn sequence_sinc_linear(polyit : ArcPoly, freq : Freq, linear_crossfade : usize) -> Vec<AudioStream<()>> {
    let (samples, arcits) = {
	let guard = polyit.lock().unwrap();
	(guard.get_samples(), guard.get())
    };
    let samplesource = Rc::new(RefCell::new(SincSampleSource::from_i8(freq, samples.as_ref())));

    let streams = vec![];

    for arcit in arcits {
	let itseq_base = Rc::new(RefCell::new(IteratorSequencer::new_with_source(arcit, freq, 1, samplesource, TrackerSensor::new())));
	let itseq : RcSyncWriter = if linear_crossfade == 0 { itseq_base.clone() } else {
	    LinearCrossfade::new_rc(linear_crossfade, itseq_base.clone())
	};
	let mut stream = AudioStream::new();
	let itseq = writer::rc_sync_observe(itseq, &stream);
    }
    //
    let itseq = sync.borrow_mut().sync(itseq.clone());
	let stereo_mapper = Rc::new(RefCell::new({let mut s = StereoMapper::new(1.0, 1.0, itseq.clone(), sen_stereo);
						  s.set_volume(vol_left, vol_right);
						  s}));
	return Rc::new(RefCell::new(SincPipeline {
	    it_proc : itseq_base.clone(),
	    stereo_mapper,
	}));
    
}
