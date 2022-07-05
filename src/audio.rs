use std::{sync::{Arc, Mutex}, thread};
use std::ops::DerefMut;
use sdl2::audio::{AudioSpec, AudioCallback};

pub use self::queue::AudioIterator;
pub use self::queue::AQOp;
pub use self::queue::AQSample;
pub use self::queue::SampleRange;

mod dsp;
mod queue;
mod samplesource;

const NOAUDIO : NoAudio = NoAudio {};
const ONE_128TH : f32 = 1.0 / 128.0;

lazy_static! {
    static ref MIXER : Mixer = init_mixer();
}

fn init_mixer() -> Mixer {
    return Mixer {
	processor : Mutex::new(AudioProcessor {
	    audio_spec : None,
	    channels : [
		ChannelState {
		    chan : CHANNELS[0],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		},
		ChannelState {
		    chan : CHANNELS[1],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		},
		ChannelState {
		    chan : CHANNELS[2],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		},
		ChannelState {
		    chan : CHANNELS[3],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		},
	    ],
	    sample_data : Vec::new(),
	})
    }
}

pub const MAX_VOLUME : f32 = 1.0;

/// Audio channel
#[derive(Clone, Copy)]
pub struct Channel {
    id : u8,
    left  : f32,
    right : f32,
}

pub const CHANNELS : [Channel;5] = [
    Channel { id: 0,
	      left : MAX_VOLUME,
	      right : 0.0,
    },
    Channel { id : 1,
	      left : 0.0,
	      right : MAX_VOLUME,
    },
    Channel { id : 2,
	      left : 0.0,
	      right : MAX_VOLUME,
    },
    Channel { id : 3,
	      left : MAX_VOLUME,
	      right : 0.0,
    },
    Channel { id : 4,
	      left : MAX_VOLUME,
	      right : MAX_VOLUME,
    },
];


struct ChannelState {
    chan : Channel,
    iterator : Arc<Mutex<dyn AudioIterator>>,
}

/// Asynchronous audio processor
/// Provides an audio callback in the main thread but defers all updates to side threads.
struct AudioProcessor {
    audio_spec : Option<AudioSpec>,
    channels : [ChannelState; 4],
    sample_data : Vec<i8>,
}

pub struct Mixer {
    processor : Mutex<AudioProcessor>,
}

impl Mixer {
}

#[allow(unused)]
impl AudioCallback for &Mixer {
    type Channel = f32;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	let mut amplitude = 0;
	let freq = mixer_audio_spec(self).freq as u32;

	{
	    let mut guard = MIXER.processor.lock().unwrap();
	    let proc = guard.deref_mut();
	    let chan = &proc.channels[0];

	    let mut guard = chan.iterator.lock().unwrap();
	    let chan_iterator = guard.deref_mut();

	    for op in chan_iterator.next() {
		match op {
		    AQOp::SetVolume(v) => {amplitude = (v * 20000.0) as i16},
		    _ => {},
		}
	    }
	}
        for x in output.iter_mut() {
	    *x = 0.0;
	}
	mixer_copy_sample(self, output, (1.0, 1.0), 0x744, 0x2fc2);
	// Clamp
        for x in output.iter_mut() {
	    let v = *x;
	    *x = f32::min(1.0, f32::max(-1.0, v));
	}
    }
}

fn mixer_audio_spec(mixer : &&Mixer) -> AudioSpec {
    let mut guard = mixer.processor.lock().unwrap();
    let proc = guard.deref_mut();
    return proc.audio_spec.unwrap()
}

fn mixer_copy_sample(mixer : &&Mixer, outbuf : &mut [f32], volume : (f32, f32), start : usize, end : usize) {
    let mut guard = mixer.processor.lock().unwrap();
    let proc = guard.deref_mut();
    let sample_data = &proc.sample_data;
    let sample = &sample_data[start..end];
    let sample_length = end - start;
    let (vol_l, vol_r) = volume;

    let mut sample_i = 0;

    for out_i in (0..outbuf.len()).step_by(2) {
	let sample_v = sample[sample_i & sample_length] as f32 * ONE_128TH;
	outbuf[out_i] = sample_v * vol_l;
	outbuf[out_i + 1] = sample_v * vol_r;
	sample_i += 1;
    }
}


impl Mixer {
    pub fn init(&'static self, spec : AudioSpec) -> &Mixer {
	let mut guard = self.processor.lock().unwrap();
	let proc = guard.deref_mut();
	let audio_spec = &mut proc.audio_spec;
	*audio_spec = Some(spec);
	return self;
    }


    pub fn set_channel(&self, c : Channel, source : Arc<Mutex<dyn AudioIterator>>) {
	mixer_set_channel(c, source);
    }
}


fn mixer_set_channel(c : Channel, source : Arc<Mutex<dyn AudioIterator>>) {
    let it = source.clone();
    let _ = thread::spawn(move || {
	let mut guard = MIXER.processor.lock().unwrap();
	let proc = guard.deref_mut();
	let channels = &mut proc.channels;
	channels[c.id as usize].iterator = it;
    });
}



struct NoAudio {}
impl AudioIterator for NoAudio {
    fn next(&mut self) -> Vec<AQOp> {
	vec![AQOp::WaitMillis(1000)]
    }
}


pub fn new(sample_data : Vec<i8>) -> &'static Mixer {
    // return Mixer {
    let mut guard = MIXER.processor.lock().unwrap();
    let proc = guard.deref_mut();
    proc.sample_data = sample_data;
    return &MIXER;
}

