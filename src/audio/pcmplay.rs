// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::{sync::Arc, rc::Rc};

/// PCM sample playing

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::dsp::writer::PCMStereoWriter;


// ================================================================================
// MonoPCM
#[derive(Clone)]
pub struct MonoPCM {
    data : Arc<Vec<f32>>,
}

impl MonoPCM {
    pub fn new(data : Arc<Vec<f32>>) -> MonoPCM {
	MonoPCM {
	    data,
	}
    }

    pub fn to_stereo(&self, left : f32, right : f32) -> StereoPCM {
	StereoPCM {
	    left : Arc::new(self.data.iter().map(|x| { x * left }).collect()),
	    right : Arc::new(self.data.iter().map(|x| { x * right }).collect()),
	}
    }
}

impl MonoPCM {
    #[allow(unused)]
    pub fn len(&self) -> usize {
	return self.data.len();
    }
}

impl From<MonoPCM> for StereoPCM {
    fn from(s: MonoPCM) -> Self {
	return s.to_stereo(1.0, 1.0);
    }
}

impl From<MonoPCM> for PCMPlayer {
    fn from(s: MonoPCM) -> Self {
	return PCMPlayer::from(StereoPCM::from(s));
    }
}

// ================================================================================
// StereoPCM

#[derive(Clone)]
pub struct StereoPCM {
    left : Arc<Vec<f32>>,
    right : Arc<Vec<f32>>,
}

impl StereoPCM {
    pub fn len(&self) -> usize {
	return self.left.len();
    }
}

impl From<StereoPCM> for PCMPlayer {
    fn from(s: StereoPCM) -> Self {
	return PCMPlayer {
	    pcm : s,
	    pos : 0,
	}
    }
}

// ================================================================================
// PCMPlayer

#[derive(Clone)]
pub struct PCMPlayer {
    pcm : StereoPCM,
    pos : usize,
}

impl PCMPlayer {
    pub fn done(&self) -> bool {
	return self.pos >= self.len();
    }

    pub fn len(&self) -> usize {
	return self.pcm.len();
    }

    fn remaining(&self) -> usize {
	return self.len() - self.pos;
    }
}

impl PCMStereoWriter for PCMPlayer {
    fn write_stereo_pcm(&mut self, output : &mut [f32]) {
	let to_write = usize::min(output.len() >> 1, self.remaining());
	let mut pos = self.pos;
	for sample_index in 0..to_write {
	    let out_pos = sample_index << 1;
	    output[out_pos + 0] = self.pcm.left[pos];
	    output[out_pos + 1] = self.pcm.right[pos];
	    pos += 1;
	}
	self.pos = pos;
    }
}

// ================================================================================
// Adapters

impl From<&[f32]> for MonoPCM {
    fn from(s: &[f32]) -> Self {
	return MonoPCM::new(Arc::new(s.to_vec()));
    }
}

impl From<Vec<f32>> for MonoPCM {
    fn from(s: Vec<f32>) -> Self {
	return MonoPCM::new(Arc::new(s));
    }
}

impl From<Rc<&[f32]>> for MonoPCM {
    fn from(s: Rc<&[f32]>) -> Self {
	return MonoPCM::new(Arc::new(s.to_vec()));
    }
}

impl From<Arc<Vec<f32>>> for MonoPCM {
    fn from(s: Arc<Vec<f32>>) -> Self {
	return MonoPCM::new(s);
    }
}

impl From<&[f32]> for StereoPCM {
    fn from(s: &[f32]) -> Self {
	return StereoPCM::from(MonoPCM::from(s));
    }
}

impl From<Vec<f32>> for StereoPCM {
    fn from(s: Vec<f32>) -> Self {
	return StereoPCM::from(MonoPCM::from(s));
    }
}

impl From<Rc<&[f32]>> for StereoPCM {
    fn from(s: Rc<&[f32]>) -> Self {
	return StereoPCM::from(MonoPCM::from(s));
    }
}

impl From<Arc<Vec<f32>>> for StereoPCM {
    fn from(s: Arc<Vec<f32>>) -> Self {
	return StereoPCM::from(MonoPCM::from(s));
    }
}
