// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::{rc::Rc, cell::RefCell};

/// Sequences song iterator channels into raw PCM audio streams that can be written
/// to a WAV file or played as-is.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use crate::audio::{ArcIt, Freq, iterator_sequencer::IteratorSequencer};

use super::{writer::RcSyncWriter, crossfade_linear::LinearCrossfade, vtracker::TrackerSensor};

struct AudioStream<T> {
    // Mono streams
    audio : Vec<f32>,
    meta : Vec<(usize, T)>, // Sorted metainformation
}

// /// Sequences an audioiterator into its constitutent (mono) streams using a sinc pipeline with
// /// optional linear cross-fade
// pub fn sequence_sinc_linear(it : ArcIt, freq : Freq, linear_crossfade : usize) -> Vec<AudioStream<()>> {
//     let itseq_base = Rc::new(RefCell::new(IteratorSequencer::new_with_source(it, freq, 1, samples.clone(), TrackerSensor::new())));
//     let itseq : RcSyncWriter = if linear_crossfade == 0 { itseq_base.clone() } else {
// 	    LinearCrossfade::new_rc(linear_crossfade, itseq_base.clone())
// 	};
// 	let itseq = sync.borrow_mut().sync(itseq.clone());
// 	let stereo_mapper = Rc::new(RefCell::new({let mut s = StereoMapper::new(1.0, 1.0, itseq.clone(), sen_stereo);
// 						  s.set_volume(vol_left, vol_right);
// 						  s}));
// 	return Rc::new(RefCell::new(SincPipeline {
// 	    it_proc : itseq_base.clone(),
// 	    stereo_mapper,
// 	}));
    
// }
