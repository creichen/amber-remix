// Copyright (C) 2024 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::{sync::{Arc, Mutex}, ops::DerefMut};
use sdl2::audio::{AudioSpec, AudioFormat, AudioCallback};

use super::Freq;

// use super::dsp::ringbuf::RingBuf;

// ================================================================================

const AUDIO_BUF_MAX_SIZE : usize = 8000;


type MixerSampleType = f32;

// ================================================================================
// AudioSource

pub trait AudioSource : Sync + Send {
    /// Returns number of samples written.
    /// If less than output.len(), this means that the audio source will be stopped afterwards.
    fn fill(&mut self, left_output: &mut [f32], right_output: &mut [f32], sample_rate: usize) -> usize;
}


// ================================================================================
// Mixer
#[derive(Clone)]
pub struct Mixer {
    pub sample_rate: usize,
    sources: Arc<Mutex<Vec<Arc<Mutex<dyn AudioSource>>>>>,
}

impl Mixer {
    fn new(sample_rate:usize) -> Self {
	Mixer {
	    sample_rate,
	    sources: Arc::new(Mutex::new(Vec::new())),
	}
    }

    pub fn add_source(&mut self, source: Arc<Mutex<dyn AudioSource>>) {
	let mut sources = self.sources.lock().unwrap();
	sources.push(source);
    }

    fn fill(&mut self, output: &mut [MixerSampleType]) {
	let mut left = [0.0; AUDIO_BUF_MAX_SIZE];
	let mut right = [0.0; AUDIO_BUF_MAX_SIZE];
	let num_samples = output.len() >> 1;
	let mut sources = self.sources.lock().unwrap();
	sources.retain(|source| {
	    let mut source = source.lock().unwrap();
	    let written = source.fill(
		&mut left[0..num_samples],
		&mut right[0..num_samples],
		self.sample_rate);
	    written == num_samples
	});
	for pos in 0..num_samples {
	    output[pos * 2] = left[pos];
	    output[pos * 2 + 1] = right[pos];
	}
    }
}

impl AudioCallback for Mixer {
    type Channel = MixerSampleType;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	trace!("Callback for {}", output.len());
	self.fill(output);
    }
}


// --------------------------------------------------------------------------------

pub struct AudioCore {
    spec : AudioSpec,
    mixer : Mixer,
    device : Option<sdl2::audio::AudioDevice<Mixer>>,
}

impl AudioCore {
    fn new() -> Self {
	AudioCore {
	    spec: AudioSpec {
		freq:    0,
		format:  AudioFormat::U8,
		channels:0,
		silence: 0,
		samples: 0,
		size:    0,
	    },
	    mixer: Mixer::new(0),
	    device: None,
	}
    }

    fn set_spec(&mut self, spec: AudioSpec) {
	self.mixer.sample_rate = spec.freq as usize;
	self.spec = spec
    }

    fn callback(&self) -> Mixer {
	self.mixer.clone()
    }

    // fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
    // 	let freq = self.spec.freq as Freq;
    // 	let mixer = Mixer::new(Arc::new(sample_data.to_vec()), freq, self.shared_buf.clone(), self.callback_tracker_sensor.clone());
    // 	self.device.as_ref().unwrap().resume();
    // 	return mixer;
    // }
}

// impl ACore {
//     pub fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
// 	let mut guard = self.ac.lock().unwrap();
// 	let cc = guard.deref_mut();
// 	return cc.start_mixer(sample_data);
//     }
// }

// ================================================================================
// ACore and SDL main hook

pub struct ACore {
    ac : Arc<Mutex<AudioCore>>,
    pub frequency : Freq,
}

impl ACore {
    fn new(ac: Arc<Mutex<AudioCore>>) -> Self {
	let frequency = {
	    let guard = ac.lock().unwrap();
	    guard.spec.freq as Freq
	};
	Self { ac, frequency }
    }

    pub fn mixer(&self) -> Mixer {
	let guard = self.ac.lock().unwrap();
	guard.mixer.clone()
    }
}

pub fn init<'a>(sdl_context : &sdl2::Sdl) -> ACore {
    let audio = sdl_context.audio().unwrap();
    let requested_audio = sdl2::audio::AudioSpecDesired {
	freq: Some(48000),
	channels: Some(2),
	samples: None
    };

    let core = Arc::new(Mutex::new(AudioCore::new()));
    let core_clone = core.clone();

    let device = audio.open_playback(None, &requested_audio, |spec| {
	let mut guard = core_clone.lock().unwrap();
	let cc = guard.deref_mut();
	cc.set_spec(spec);
	cc.callback()
    }).unwrap();

    {
	let mut guard = core.lock().unwrap();
	let cc = guard.deref_mut();
	device.resume();
	cc.device = Some(device);
    }


    ACore::new(core)
}

impl ACore {
    // pub fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
    // 	let mut guard = self.ac.lock().unwrap();
    // 	let cc = guard.deref_mut();
    // 	return cc.start_mixer(sample_data);
    // }
}
