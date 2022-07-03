use std::{sync::{Arc, Mutex}, cell::RefCell, rc::Rc, ops::Range, thread};
use std::ops::DerefMut;

use sdl2::audio::{AudioSpec, AudioCallback};

pub const MAX_VOLUME : u16 = 0xffff;

/// Audio channel
#[derive(Clone, Copy)]
pub struct Channel {
    left  : u16,
    right : u16,
    nr : u8,
}

pub const CHANNELS : [Channel;4] = [
    Channel { left : MAX_VOLUME,
	      right : 0,
	      nr : 0 },
    Channel { left : 0,
	      right : MAX_VOLUME,
	      nr : 1 },
    Channel { left : 0,
	      right : MAX_VOLUME,
	      nr : 2 },
    Channel { left : MAX_VOLUME,
	      right : 0,
	      nr : 3 },
];


pub struct SampleRange {
    pub range : Range<usize>,
}

pub enum Sample {
    /// Loop specified sample
    Loop(SampleRange),
    /// Play specified sample once
    Once(SampleRange),
}

/**
 * Audio queue operations allow AudioIterators to control output to their channel.
 *
 * "X ; WaitMillis(n); Y" means that settings X will be in effect for "n" milliseconds,
 * then any changes from Y take effect.
 */
pub enum AudioQueueOp {
    /// Process channel settings for specified nr of milliseconds
    WaitMillis(usize),
    /// Enqueue to the sample queue
    SetSamples(Vec<Sample>),
    /// Set audio frequency in Hz
    SetFreq(f32),
    /// Set audio volume as fraction
    SetVolume(f32),
}

pub trait AudioIterator : Send + Sync {
    fn next(&mut self) -> Vec<AudioQueueOp>;
}

struct ChannelState {
    chan : Channel,
    iterator : Arc<dyn AudioIterator>,
    sample : Vec<Sample>,
    freq : f32,
    volume : f32,
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

impl AudioCallback for &Mixer {
    type Channel = i16;

    fn callback(&mut self, output: &mut [Self::Channel]) {
        for x in output.iter_mut() {
	    *x = 0;
	}
    }
}

impl Mixer {
    pub fn init(&'static self, spec : AudioSpec) {
	let mut guard = self.processor.lock().unwrap();
	let proc = guard.deref_mut();
	let audio_spec = &mut proc.audio_spec;
	*audio_spec = Some(spec);
    }

}

pub fn set_channel(&mixer : Mixer, c : Channel, source : &dyn AudioIterator) {
    thread::spawn(move || {
	let mut guard = self.processor.lock().unwrap();
	let proc = guard.deref_mut();
	let audio_spec = &mut proc.audio_spec;
	
    });
}


struct NoAudio {}
impl AudioIterator for NoAudio {
    fn next(&mut self) -> Vec<AudioQueueOp> {
	vec![AudioQueueOp::WaitMillis(1000)]
    }
}

pub fn new(sample_data : Vec<i8>) -> Mixer {
    return Mixer {
	processor : Mutex::new(AudioProcessor {
	    audio_spec : None,
	    channels : [
		ChannelState {
		    chan : CHANNELS[0],
		    iterator : Arc::new(NoAudio{}),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[1],
		    iterator : Arc::new(NoAudio{}),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[2],
		    iterator : Arc::new(NoAudio{}),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[3],
		    iterator : Arc::new(NoAudio{}),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
	    ],
	    sample_data,
	})
    }
}

