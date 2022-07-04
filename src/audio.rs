use std::{sync::{Arc, Mutex}, ops::Range, thread};
use std::ops::DerefMut;
use sdl2::audio::{AudioSpec, AudioCallback};

const NOAUDIO : NoAudio = NoAudio {};

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
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[1],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[2],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
		ChannelState {
		    chan : CHANNELS[3],
		    iterator : Arc::new(Mutex::new(NOAUDIO)),
		    sample : Vec::new(),
		    freq : 1.0,
		    volume : 1.0,
		},
	    ],
	    sample_data : Vec::new(),
	})
    }
}

pub const MAX_VOLUME : u16 = 0xffff;

/// Audio channel
#[derive(Clone, Copy)]
pub struct Channel {
    id : u8,
    left  : u16,
    right : u16,
}

pub const CHANNELS : [Channel;4] = [
    Channel { id: 0,
	      left : MAX_VOLUME,
	      right : 0,
    },
    Channel { id : 1,
	      left : 0,
	      right : MAX_VOLUME,
    },
    Channel { id : 2,
	      left : 0,
	      right : MAX_VOLUME,
    },
    Channel { id : 3,
	      left : MAX_VOLUME,
	      right : 0,
    },
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
    iterator : Arc<Mutex<dyn AudioIterator>>,
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

impl Mixer {
    fn get_freq(&mut self) -> u32 {
	let mut guard = self.processor.lock().unwrap();
	let proc = guard.deref_mut();
	return proc.audio_spec.unwrap().freq as u32;
    }
}

impl AudioCallback for Mixer {
    type Channel = i16;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	let mut amplitude = 0;
	let freq = self.get_freq();
	println!("{}", freq);

	// {
	//     let mut guard = self.processor.lock().unwrap();
	//     let proc = guard.deref_mut();
	//     return proc.audio_spec.unwrap().freq as u32;
	// }

	{
	    let mut guard = self.processor.lock().unwrap();
	    let proc = guard.deref_mut();
	    let chan = &proc.channels[0];

	    let mut guard = chan.iterator.lock().unwrap();
	    let chan_iterator = guard.deref_mut();

	    for op in chan_iterator.next() {
		match op {
		    AudioQueueOp::SetVolume(v) => {amplitude = (v * 20000.0) as i16},
		    _ => {},
		}
	    }
	}
	let mut pos = 0;
        for x in output.iter_mut() {
	    if pos & 1 == 0 {
		*x = if (pos % 100) < 50 {amplitude} else {-amplitude};
	    } else {
		*x = 0;
	    }
	    pos += 1;
	}
    }
}

// struct CallerBacker {}
// static CB : CallerBacker = CallerBacker {};
// impl CallerBacker {
//     pub fn set_channel(&'static self, mixer : &mut Mixer, c : Channel, source : &dyn AudioIterator) {
// 	thread::spawn(move || {
// 	let mut guard = mixer.processor.lock().unwrap();
// 	let proc = guard.deref_mut();
// 	let audio_spec = &mut proc.audio_spec;
	
//     });
// }
    
// }


impl Mixer {
    pub fn init(&'static self, spec : AudioSpec) -> MixerCallback {
	let mut guard = self.processor.lock().unwrap();
	let proc = guard.deref_mut();
	let audio_spec = &mut proc.audio_spec;
	*audio_spec = Some(spec);
    }


    pub fn set_channel(&self, c : Channel, source : Arc<Mutex<dyn AudioIterator>>) {
	// let it = source.clone();
	// let _ = thread::spawn(move || {
	//     let mut guard = self.processor.lock().unwrap();
	//     let proc = guard.deref_mut();
	//     let channels = &mut proc.channels;
	//     channels[c.id as usize].iterator = it;
	// });
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
    fn next(&mut self) -> Vec<AudioQueueOp> {
	vec![AudioQueueOp::WaitMillis(1000)]
    }
}


pub fn new(sample_data : Vec<i8>) -> &'static Mixer {
    // return Mixer {
    let mut guard = MIXER.processor.lock().unwrap();
    let proc = guard.deref_mut();
    proc.sample_data = sample_data;
    return &MIXER;
}


