use std::{sync::{Arc, Mutex, mpsc::{self, Sender, Receiver}}, thread, collections::VecDeque, rc::Rc, cell::RefCell};
use std::ops::DerefMut;
use sdl2::audio::{AudioSpec, AudioCallback, AudioFormat};

use self::{queue::AudioIteratorProcessor, samplesource::SampleSource};
#[allow(unused)]
use self::{dsp::{linear::LinearFilter, stereo_mapper::StereoMapper, frequency_range::Freq, writer::FlexPCMWriter}, queue::AudioQueue, samplesource::SimpleSampleSource};
pub use self::iterator::AudioIterator;
pub use self::iterator::MockAudioIterator;
pub use self::iterator::AQOp;
pub use self::iterator::AQSample;
pub use self::queue::SampleRange;
pub use self::iterator::ArcIt;

mod dsp;
mod queue;
mod iterator;
mod samplesource;

const NUM_CHANNELS : usize = 5;

const AUDIO_BUF_DEFAULT_SIZE : usize = 16384;
const AUDIO_BUF_MAX_SIZE : usize = 16384;
pub const MAX_VOLUME : f32 = 1.0;

// ================================================================================
// Filter

struct LinearFilteringPipeline {
    it_proc : Rc<RefCell<dyn AudioIteratorProcessor>>,
    aqueue : Rc<RefCell<dyn FlexPCMWriter>>,
    linear_filter : Rc<RefCell<LinearFilter>>,
    stereo_mapper : Rc<RefCell<StereoMapper>>,
}

impl LinearFilteringPipeline {
    fn new(it : ArcIt, sample_source : Rc<dyn SampleSource>, output_freq : Freq) -> LinearFilteringPipeline {
	let aqueue = Rc::new(RefCell::new(AudioQueue::new(it, sample_source)));
	let linear_filter = Rc::new(RefCell::new(LinearFilter::new(40000, output_freq, aqueue.clone())));
	let stereo_mapper = Rc::new(RefCell::new(StereoMapper::new(1.0, 1.0, linear_filter.clone())));
	return LinearFilteringPipeline {
	    it_proc : aqueue.clone(),
	    aqueue,
	    linear_filter,
	    stereo_mapper,
	}
    }

    fn set_iterator(&mut self, it : ArcIt) {
	self.it_proc.borrow_mut().set_source(it);
    }
}

// ================================================================================
// OutputBuffer

const OUTPUT_BUFFER_IS_FULL : usize = 0xffffffff;

// Ring buffer semantics
struct OutputBuffer {
    last_poll : usize,
    write_pos : usize, // may be OUTPUT_BUFFER_IS_FULL
    read_pos : usize,
    data : [f32; AUDIO_BUF_MAX_SIZE * 2],
}

impl OutputBuffer {
    fn new() -> OutputBuffer {
	OutputBuffer {
	    last_poll : AUDIO_BUF_MAX_SIZE,
	    write_pos : 0,
	    read_pos : 0,
	    data : [0.0; AUDIO_BUF_MAX_SIZE * 2],
	}
    }

    pub fn capacity(&self) -> usize {
	return self.data.len();
    }

    pub fn is_full(&self) -> bool {
	return self.write_pos == OUTPUT_BUFFER_IS_FULL;
    }

    pub fn len(&self) -> usize {
	let cap = self.capacity();
	if self.is_full() {
	    cap;
	};
	if self.write_pos < self.read_pos {
	    self.write_pos + cap - self.read_pos;
	}
	return self.write_pos - self.read_pos;
    }

    fn can_read_to_end_of_buffer(&self) -> bool{
	return self.is_full() || self.read_pos > self.write_pos;
    }

    fn write_to(&mut self, dest : &mut [f32]) -> usize {
	let initially_available = self.len();
	let requested = dest.len();
	let read_end_pos = if self.can_read_to_end_of_buffer() { self.capacity() } else { self.write_pos };
	let avail = read_end_pos - self.read_pos;

	let to_write = usize::min(avail, requested);

	if to_write > 0 {
	    dest.copy_from_slice(&self.data[self.read_pos..self.read_pos + avail]);

	    if self.is_full() {
		self.write_pos = self.read_pos;
	    }
	    self.read_pos += to_write;
	}
	// We might be done now
	if to_write == requested || to_write == initially_available {
	    return requested;
	}
	// Otherwise, we must have hit the end of the buffer
	// Call ourselves one final time to finish up
	self.read_pos -= self.capacity();
	return to_write + self.write_to(&mut dest[to_write..]);
    }

    fn read_from(&mut self, src : &[f32]) -> usize {
	if self.is_full() {
	    return 0;
	}
	let initially_available = self.capacity() - self.len();
	let requested = src.len();
	let write_start_pos = self.write_pos;
	let write_end_pos = if self.read_pos <= write_start_pos { self.capacity() } else { self.read_pos };
	let avail = write_end_pos - write_start_pos;

	let to_write = usize::min(avail, requested);

	if to_write > 0 {
	    self.data[write_start_pos..write_start_pos+to_write].copy_from_slice(&src[0..to_write]);

	    self.write_pos += to_write;
	    if self.write_pos == self.read_pos {
		self.write_pos = OUTPUT_BUFFER_IS_FULL;
	    }
	}
	// We might be done now
	if to_write == requested || to_write == initially_available {
	    return requested;
	}
	// Otherwise, we must have hit the end of the buffer
	// Call ourselves one final time to finish up
	self.write_pos -= self.capacity();
	return to_write + self.read_from(&src[to_write..]);

    }
}

// ================================================================================
// Callback

struct Callback {
    spec : AudioSpec,
    shared_buf : Arc<Mutex<OutputBuffer>>,
}

impl AudioCallback for Callback {
    type Channel = f32;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	let mut guard = self.shared_buf.lock().unwrap();
	let buf = guard.deref_mut();
	buf.last_poll = output.len();
	let num_written = buf.write_to(output);
	if num_written < output.len() {
	    println!("[Audio] Buffer underrun: {num_written}/{}", output.len())
	}
	for x in output[num_written..].iter_mut() {
	    *x = 0.0;
	}
    }
}

impl Callback {
    fn new(spec : AudioSpec, shared_buf : Arc<Mutex<OutputBuffer>>) -> Callback {
	return Callback {
	    spec, shared_buf,
	}
    }
}

// ================================================================================
// AudioCore

pub struct AudioCore {
    spec : AudioSpec,
    shared_buf : Arc<Mutex<OutputBuffer>>,
    device : Option<sdl2::audio::AudioDevice<Callback>>,
}

impl AudioCore {
    fn init(&mut self, spec : AudioSpec) -> Callback {
	self.spec = spec;
	return Callback::new(self.spec, self.shared_buf.clone());
    }

    pub fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
	let freq = self.spec.freq as Freq;
	let mixer = Mixer::new(Arc::new(sample_data.to_vec()), freq, self.shared_buf.clone());
	self.device.as_ref().unwrap().resume();
	return mixer;
    }
}

impl ACore {
    pub fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
	let mut guard = self.ac.lock().unwrap();
	let cc = guard.deref_mut();
	return cc.start_mixer(sample_data);
    }
}

// ================================================================================
// ACore and SDL main hook

pub struct ACore {
    ac : Arc<Mutex<AudioCore>>,
}

pub fn init<'a>(sdl_context : &sdl2::Sdl) -> ACore {
    let audio = sdl_context.audio().unwrap();
    let requested_audio = sdl2::audio::AudioSpecDesired {
	freq: Some(44100),
	channels: Some(2),
	samples: None
    };

    let core = Arc::new(Mutex::new(AudioCore {
	spec : AudioSpec {
	    freq: 0,
	    format: AudioFormat::U8,
	    channels:0,
	    silence: 0,
	    samples: 0,
	    size: 0,
	},
	shared_buf : Arc::new(Mutex::new(OutputBuffer::new())),
	device : None,
    }));
    let core_clone = core.clone();

    let device = audio.open_playback(None, &requested_audio, |spec| {
	let mut guard = core_clone.lock().unwrap();
	let cc = guard.deref_mut();
	return cc.init(spec);
    }).unwrap();

    {
	let mut guard = core.lock().unwrap();
	let cc = guard.deref_mut();
	cc.device = Some(device);
    }

    return ACore {
	ac : core
    };
}

// ================================================================================
// Mixer

pub struct Mixer {
    iterator_updates : Arc<Mutex<Vec<ArcIt>>>,
    control_channel : Sender<u8>,
}

impl Mixer {
    fn new(samples : Arc<Vec<i8>>, freq : Freq, out_buf : Arc<Mutex<OutputBuffer>>) -> Mixer {
	let iterator_updates = Arc::new(Mutex::new(Vec::new()));
	let iterator_clone = iterator_updates.clone();
	let (tx, rx) = mpsc::channel();

	let _ = thread::spawn(move || {
	    run_mixer_thread(freq, samples, out_buf.clone(), iterator_clone, rx);
	});

	return Mixer {
	    iterator_updates : iterator_updates.clone(),
	    control_channel : tx,
	}
    }
    pub fn set_iterator(&mut self, it : ArcIt) {
	let mut guard = self.iterator_updates.lock().unwrap();
	let v = guard.deref_mut();
	v.push(it.clone());
    }
}

// ================================================================================
// MixerThread

struct MixerThread {
    pipeline : LinearFilteringPipeline,
    control_channel : Receiver<u8>,
    buf : Arc<Mutex<OutputBuffer>>,
    arcit_updates : Arc<Mutex<Vec<ArcIt>>>,
}

fn run_mixer_thread(freq : Freq,
		    samples : Arc<Vec<i8>>,
		    buf : Arc<Mutex<OutputBuffer>>,
		    arcit_updates : Arc<Mutex<Vec<ArcIt>>>,
		    control_channel : Receiver<u8>)
{
    let sample_source = Rc::new(SimpleSampleSource::from_iter(samples.iter()));
    let pipeline = LinearFilteringPipeline::new(iterator::silent(), sample_source, freq);
    let mut mt = MixerThread {
	pipeline,
	control_channel,
	buf,
	arcit_updates,
    };
    mt.run();
}

impl MixerThread {
    fn run(&mut self) {
	loop{}
    }
}


// ================================================================================
// Mixer



// struct ChannelState {
//     chan : Channel,
//     iterator_new : bool,
//     iterator : ArcIt,
// }

// impl ChannelState {
//     fn init(&mut self, sample_source : Rc<dyn SampleSource>, freq : Freq) {
// 	let it = self.iterator.clone();
// 	self.pipeline.borrow_mut() = Some(LinearFilteringPipeline::new(it, sample_source, freq));
//     }

//     fn set_iterator(&mut self, it : ArcIt) {
// 	self.iterator = it.clone();
// 	match self.pipeline.borrow() {
// 	    None => {},
// 	    Some( p ) => p.set_iterator(it.clone()),
// 	}
//     }
// }


// /// Asynchronous audio processor
// /// Provides an audio callback in the main thread but defers all updates to side threads.
// struct AudioProcessor {
//     audio_spec : Option<AudioSpec>,
//     channels : [ChannelState; NUM_CHANNELS],
//     pipelines : [Box<Option<LinearFilteringPipeline>>; NUM_CHANNELS],
//     sample_data : Vec<i8>,
// }

// pub struct Mixer {
//     processor : Mutex<AudioProcessor>,
// }

// impl Mixer {
// }

// #[allow(unused)]
// impl AudioCallback for &Mixer {
//     type Channel = f32;

//     fn callback(&mut self, output: &mut [Self::Channel]) {
// 	let mut amplitude = 0;
// 	let freq = mixer_audio_spec(self).freq as u32;

// 	{
// 	    let mut guard = MIXER.processor.lock().unwrap();
// 	    let proc = guard.deref_mut();
// 	    let chan = &proc.channels[0];

// 	    let mut guard = chan.iterator.lock().unwrap();
// 	    let chan_iterator = guard.deref_mut();

// 	    // for op in chan_iterator.next() {
// 	    // 	match op {
// 	    // 	    AQOp::SetVolume(v) => {amplitude = (v * 20000.0) as i16},
// 	    // 	    _ => {},
// 	    // 	}
// 	    // }
// 	}
//         for x in output.iter_mut() {
// 	    *x = 0.0;
// 	}
// 	mixer_copy_sample(self, output, (1.0, 1.0), 0x744, 0x2fc2);
// 	// Clamp
//         for x in output.iter_mut() {
// 	    let v = *x;
// 	    *x = f32::min(1.0, f32::max(-1.0, v));
// 	}
//     }
// }

// fn mixer_audio_spec(mixer : &&Mixer) -> AudioSpec {
//     let mut guard = mixer.processor.lock().unwrap();
//     let proc = guard.deref_mut();
//     return proc.audio_spec.unwrap()
// }

// fn mixer_copy_sample(mixer : &&Mixer, outbuf : &mut [f32], volume : (f32, f32), start : usize, end : usize) {
//     let mut guard = mixer.processor.lock().unwrap();
//     let proc = guard.deref_mut();
//     let sample_data = &proc.sample_data;
//     let sample = &sample_data[start..end];
//     let sample_length = end - start;
//     let (vol_l, vol_r) = volume;

//     let mut sample_i = 0;

//     for out_i in (0..outbuf.len()).step_by(2) {
// 	let sample_v = sample[sample_i & sample_length] as f32 * ONE_128TH;
// 	outbuf[out_i] = sample_v * vol_l;
// 	outbuf[out_i + 1] = sample_v * vol_r;
// 	sample_i += 1;
//     }
// }


// impl Mixer {
//     pub fn init(&'static self, spec : AudioSpec) -> &Mixer {
// 	let mut guard = self.processor.lock().unwrap();
// 	let proc = guard.deref_mut();
// 	let audio_spec = &mut proc.audio_spec;
// 	*audio_spec = Some(spec);
// 	return self;
//     }


//     pub fn set_channel(&self, c : Channel, source : ArcIt) {
// 	mixer_set_channel(c, source);
//     }
// }


// fn mixer_set_channel(c : Channel, source : ArcIt) {
//     let it = source.clone();
//     let _ = thread::spawn(move || {
// 	let mut guard = MIXER.processor.lock().unwrap();
// 	let proc = guard.deref_mut();
// 	let channels = &mut proc.channels;
// 	channels[c.id as usize].iterator = Some(it);
// 	channels[c.id as usize].iterator_new = true;
//     });
// }



// pub fn new(sample_data : Vec<i8>) -> &'static Mixer {
//     // return Mixer {
//     let mut guard = MIXER.processor.lock().unwrap();
//     let proc = guard.deref_mut();
//     proc.sample_data = sample_data;
//     return &MIXER;
// }

