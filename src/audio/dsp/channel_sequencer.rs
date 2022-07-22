// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Sequences song iterator channels into raw PCM audio streams that can be written
/// to a WAV file or played as-is.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::{rc::Rc, cell::RefCell, sync::{Arc, Mutex}};
use crate::audio::{Freq, iterator_sequencer::IteratorSequencer, iterator::{ArcPoly, AudioIteratorObserver, self}, samplesource::SincSampleSource, pcmplay::{MonoPCM, StereoPCM}};
use super::{writer::{RcSyncWriter, self, PCMSyncObserver, Timeslice}, crossfade_linear::LinearCrossfade, vtracker::TrackerSensor, pcmsync};

// ================================================================================
// AudioStream

#[derive(Clone)]
pub struct AudioStream<T> {
    pub audio : Vec<f32>,       // Mono stream
    pub meta : Vec<(usize, T)>, // Sorted metainformation
}

impl<T> AudioStream<T> {
    fn new<U>() -> AudioStream<U> {
	return AudioStream {
	    audio : vec![],
	    meta : vec![],
	}
    }

    // fn pos(&self) -> usize {
    // 	self.audio.len()
    // }
}

impl<T> From<&AudioStream<T>> for MonoPCM {
    fn from(astream: &AudioStream<T>) -> Self {
	MonoPCM::new(Arc::new(astream.audio[..].to_vec()))
    }
}

impl<T> From<&AudioStream<T>> for StereoPCM {
    fn from(astream: &AudioStream<T>) -> Self {
	StereoPCM::from(MonoPCM::from(astream))
    }
}

// ================================================================================
// debug meta-information

#[derive(Clone)]
pub struct ASAnnotation {
    pub subsys : &'static str,
    pub event  : &'static str,
    pub data  : String,
}

#[derive(Clone)]
pub enum ASMeta {
    A(ASAnnotation),
    TS(Timeslice),
}

impl AudioStream<ASMeta> {
    pub fn find_tick(&self, t : Timeslice) -> Option<usize> {
	for (pos, meta) in self.meta.iter() {
	    if let ASMeta::TS(t2) = meta {
		if *t2 == t {
		    return Some(*pos);
		}
	    }
	}
	return None;
    }
}

// ================================================================================
// sequencing helpers

struct AStreamCollector<T> {
    astream : AudioStream<T>,
    done : bool,
}

impl<T> AStreamCollector<T> {
    pub fn new() -> AStreamCollector<T> {
	AStreamCollector {
	    astream : AudioStream::<T>::new(),
	    done : false,
	}
    }
}

impl AStreamCollector<ASMeta> {
    pub fn record_meta(&mut self, subsys: &'static str, event : &'static str, data : String) {
	let record = ASMeta::A(ASAnnotation { subsys, event, data });
	let pos = self.astream.audio.len();
	self.astream.meta.push((pos, record));
    }

    pub fn record_tick(&mut self, t : Timeslice) {
	let record = ASMeta::TS(t);
	let pos = self.astream.audio.len();
	self.astream.meta.push((pos, record));
    }
}

#[derive(Clone)]
struct AStreamPCMSyncObserver<T> {
    asc : Arc<Mutex<AStreamCollector<T>>>,
    max_tick : usize,
}

impl<T> AStreamPCMSyncObserver<T> {
    fn new() -> AStreamPCMSyncObserver<T> {
	AStreamPCMSyncObserver {
	    asc : Arc::new(Mutex::new(AStreamCollector::<T>::new())),
	    max_tick : usize::max_value(),
	}
    }

    // // Returns whether we are done
    // fn read1(&self, buf : &mut [f32]) -> bool {
    // 	self.source.borrow_mut().write_pcm(&mut buf);
    // 	self.stream.audio.extend_from_slice(&buf);
    // 	return self.done;
    // }

    // // Returns whether we are done
    // fn read1(&self, buf : &mut [f32]) {
    // 	self.source.borrow_mut().write_pcm(buf);
    // }

    // Returns whether we are done
    fn finish_read(&mut self, buf : &[f32]) -> bool {
	let mut guard = self.asc.lock().unwrap();
	guard.astream.audio.extend_from_slice(&buf);
	return guard.done;
    }

    // // Returns whether we are done
    // fn read3(&self) -> bool {
    // 	let mut buf = [0.0; 1];
    // 	self.source.borrow_mut().write_pcm(&mut buf);
    // 	self.stream.audio.extend_from_slice(&buf);
    // 	return self.done;
    // }
}

impl PCMSyncObserver for AStreamPCMSyncObserver<()> {
    fn observe_write(&mut self, _result : writer::SyncPCMResult, _written : &[f32]) {
    }

    fn observe_sync(&mut self, timeslice : writer::Timeslice) {
	println!("Tick {timeslice}");
	if timeslice >= self.max_tick {
	    let mut guard = self.asc.lock().unwrap();
	    guard.done = true;
	}
    }
}

impl AudioIteratorObserver for AStreamCollector<()> {
    fn observe_aqop(&mut self, result : &crate::audio::AQOp) {
	match result {
	    // crate::audio::AQOp::WaitMillis(_) => todo!(),
	    // crate::audio::AQOp::Timeslice(_) => todo!(),
	    // crate::audio::AQOp::SetSamples(_) => todo!(),
	    // crate::audio::AQOp::SetFreq(_) => todo!(),
	    // crate::audio::AQOp::SetVolume(_) => todo!(),
	    crate::audio::AQOp::End => { self.done = true; },
	    _ => {},
	}
    }
}

impl PCMSyncObserver for AStreamPCMSyncObserver<ASMeta> {
    fn observe_write(&mut self, wr_result : writer::SyncPCMResult, _written : &[f32]) {
	let mut guard = self.asc.lock().unwrap();
	match wr_result {
	    writer::SyncPCMResult::Wrote(n, None)     => guard.record_meta("pcms", "Wrote", format!("({n})")),
	    writer::SyncPCMResult::Wrote(n, Some(ts)) => { guard.record_tick(ts);
							   guard.record_meta("pcms", "Wrote", format!("({n}, ts:{ts})"));
	                                                 },
	    writer::SyncPCMResult::Flush              => guard.record_meta("pcms", "Flush", format!("")),
	}
    }

    fn observe_sync(&mut self, timeslice : writer::Timeslice) {
	let mut guard = self.asc.lock().unwrap();
	guard.record_meta("pcms", "Timeslice", format!("{timeslice}"));
	if timeslice >= self.max_tick {
	    guard.done = true;
	}
    }
}

impl AudioIteratorObserver for AStreamCollector<ASMeta> {
    fn observe_aqop(&mut self, result : &crate::audio::AQOp) {
	match result {
	    crate::audio::AQOp::WaitMillis(ms) => self.record_meta("arcit", "WaitMillis", format!("{ms}")),
	    crate::audio::AQOp::Timeslice(ts)  => self.record_meta("arcit", "Timeslice", format!("{ts}")),
	    crate::audio::AQOp::SetSamples(sv) => self.record_meta("arcit", "SetSamples", format!("{sv:?}")),
	    crate::audio::AQOp::SetFreq(freq)  => self.record_meta("arcit", "SetFreq", format!("{freq}")),
	    crate::audio::AQOp::SetVolume(vol) => self.record_meta("arcit", "SetVolume", format!("{vol}")),
	    crate::audio::AQOp::End            => { self.record_meta("arcit", "End", format!(""));
		                                    self.done = true; },
	}
    }
}

// ================================================================================
// sequencing

/// Sequences an audioiterator into its constitutent (mono) streams using a sinc pipeline with
/// optional linear cross-fade
pub fn sequence_sinc_linear(polyit : ArcPoly, freq : Freq, maxtick : Option<usize>, linear_crossfade : usize) -> Vec<AudioStream<ASMeta>> {
    let (samples, arcits) = {
	let mut guard = polyit.lock().unwrap();
	(guard.get_samples(), guard.get())
    };
    let samplesource = Rc::new(RefCell::new(SincSampleSource::from_i8(freq, samples.as_ref())));
    let sync = pcmsync::new_basic();

    let mut observers = vec![];

    for arcit in arcits {
	let (pcm_observer, arcit_observer) = {
	    let mut po = AStreamPCMSyncObserver::<ASMeta>::new();
	    if let Some(limit) = maxtick {
		po.max_tick = limit;
	    }
	    (Rc::new(RefCell::new(po.clone())), po.asc.clone())
	};
	let arcit = iterator::observe(arcit, arcit_observer);
	let itseq_base = Rc::new(RefCell::new(IteratorSequencer::new_with_source(arcit, freq, 1, samplesource.clone(), TrackerSensor::new())));
	let itseq : RcSyncWriter = if linear_crossfade == 0 { itseq_base.clone() } else {
	    LinearCrossfade::new_rc(linear_crossfade, itseq_base.clone())
	};
	let itseq = writer::observe_rc_sync(itseq, pcm_observer.clone());
	let itseq = sync.borrow_mut().sync(itseq.clone());
	observers.push((itseq.clone(), pcm_observer));
    }

    let mut done = false;
    while !done {
	done = true;
	for (source, observer) in observers.iter_mut() {
	    // source.
	    let mut buf = [0.0; 1];
	    source.borrow_mut().write_pcm(&mut buf);
	    // observer.borrow().read1(&mut buf);
	    done &= observer.borrow_mut().finish_read(&buf);
	}
    };

    let mut result = vec![];
    for (_, observer) in observers.drain(..) {
	let olock = observer.borrow();
	let guard = olock.asc.lock().unwrap();
	result.push(guard.astream.clone());
    }

    return result;
}
