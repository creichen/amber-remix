use core::{time, borrow};
use std::{sync::{Arc, Mutex, mpsc::{self, Sender, Receiver}}, thread, rc::Rc, cell::RefCell};
//use std::{sync::{Arc, Mutex, mpsc::{self, Sender, Receiver}}, thread, rc::Rc, cell::RefCell, borrow::BorrowMut};
use std::ops::DerefMut;
use log::{trace, debug, log_enabled, Level, warn, info};
use sdl2::audio::{AudioSpec, AudioCallback, AudioFormat};

use self::{queue::AudioIteratorProcessor, samplesource::SampleSource, dsp::vtracker::{TrackerSensor, Tracker}};
#[allow(unused)]
use self::{dsp::{linear::LinearFilter, stereo_mapper::StereoMapper, writer::FlexPCMWriter}, queue::AudioQueue, samplesource::SimpleSampleSource};
pub use self::iterator::AudioIterator;
pub use self::iterator::MockAudioIterator;
pub use self::iterator::AQOp;
pub use self::iterator::AQSample;
pub use self::queue::SampleRange;
pub use self::dsp::frequency_range::Freq;
pub use self::iterator::ArcIt;

mod dsp;
mod queue;
mod iterator;
mod samplesource;

const AUDIO_BUF_DEFAULT_POLL_SIZE : usize = 8192; // # of bytes polled by the audio subsystem
const AUDIO_BUF_MAX_SIZE : usize = 16384;

// ================================================================================
// Filter

struct LinearFilteringPipeline {
    it_proc : Rc<RefCell<dyn AudioIteratorProcessor>>,
    aqueue : Rc<RefCell<dyn FlexPCMWriter>>,
    linear_filter : Rc<RefCell<LinearFilter>>,
    stereo_mapper : Rc<RefCell<StereoMapper>>,
}

impl LinearFilteringPipeline {
    fn new(it : ArcIt, sample_source : Rc<dyn SampleSource>, output_freq : Freq,
	   sen_queue : TrackerSensor, sen_linear : TrackerSensor, sen_stereo : TrackerSensor) -> LinearFilteringPipeline {
	let aqueue = Rc::new(RefCell::new(AudioQueue::new(it, sample_source, sen_queue)));
	let linear_filter = Rc::new(RefCell::new(LinearFilter::new(40000, output_freq, aqueue.clone(), sen_linear)));
	let stereo_mapper = Rc::new(RefCell::new(StereoMapper::new(1.0, 1.0, linear_filter.clone(), sen_stereo)));
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
	    last_poll : AUDIO_BUF_DEFAULT_POLL_SIZE,
	    write_pos : 0,
	    read_pos : 0,
	    data : [0.0; AUDIO_BUF_MAX_SIZE * 2],
	}
    }

    pub fn capacity(&self) -> usize {
	return self.data.len();
    }

    pub fn remaining_capacity(&self) -> usize {
	return self.capacity() - self.len();
    }

    pub fn is_full(&self) -> bool {
	return self.write_pos == OUTPUT_BUFFER_IS_FULL;
    }

    pub fn is_empty(&self) -> bool {
	return self.len() == 0;
    }

    pub fn len(&self) -> usize {
	let cap = self.capacity();
	if self.is_full() {
	    return cap;
	};
	if self.write_pos < self.read_pos {
	    return self.write_pos + cap - self.read_pos;
	}
	return self.write_pos - self.read_pos;
    }

    /// How much are we expecting to read from here?
    pub fn expected_read(&self) -> usize {
	return self.last_poll;
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
	    dest[0..to_write].copy_from_slice(&self.data[self.read_pos..self.read_pos + to_write]);

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
    tracker : TrackerSensor,
}

impl AudioCallback for Callback {
    type Channel = f32;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	let mut guard = self.shared_buf.lock().unwrap();
	let buf = guard.deref_mut();
	buf.last_poll = output.len();
	let num_written = buf.write_to(output);

	if num_written < output.len() {
	    warn!("Buffer underrun {num_written}/{}", output.len());
	}
	for x in output[num_written..].iter_mut() {
	    *x = 0.0;
	}
	let mut debug_total = 0.0;
	for x in output[0..num_written].iter() {
	    debug_total += f32::abs(*x);
	}
	self.tracker.add_many(debug_total, output.len());
	// if log_enabled!(Level::Debug) {
	//     let mut debug_total : u64 = 0;
	//     print!("[AuadioCallback--Final] output [");
	//     let mut pos = 0;
	//     for x in output[0..num_written].iter() {
	// 	debug_total += f32::abs(x * 5.0) as u64;
	// 	pos += 1;
	// 	if (pos & 0x1f) == 0 || pos == num_written {
	// 	    let v = debug_total;
	// 	    let c = if v < 1 { " " }
	// 	    else if v < 10 { "." }
	// 	    else if v < 30 { "_" }
	// 	    else if v < 60 { "=" }
	// 	    else if v < 100 { "*" }
	// 	    else { "#" };
	// 	    print!("{c}");
	// 	    debug_total = 0;
	// 	}
	//     }
	//     println!("]");
	// }

    }
}

impl Callback {
    fn new(spec : AudioSpec, shared_buf : Arc<Mutex<OutputBuffer>>, tracker : TrackerSensor) -> Callback {
	return Callback {
	    spec, shared_buf, tracker,
	}
    }
}

// ================================================================================
// AudioCore

pub struct AudioCore {
    spec : AudioSpec,
    shared_buf : Arc<Mutex<OutputBuffer>>,
    device : Option<sdl2::audio::AudioDevice<Callback>>,
    callback_tracker_sensor : TrackerSensor,
}

impl AudioCore {
    fn init(&mut self, spec : AudioSpec) -> Callback {
	self.spec = spec;
	return Callback::new(self.spec, self.shared_buf.clone(), self.callback_tracker_sensor.clone());
    }

    pub fn start_mixer<'a>(&mut self, sample_data : &'a [i8]) -> Mixer {
	let freq = self.spec.freq as Freq;
	let mixer = Mixer::new(Arc::new(sample_data.to_vec()), freq, self.shared_buf.clone(), self.callback_tracker_sensor.clone());
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
	callback_tracker_sensor : TrackerSensor::new(),
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

#[derive(Clone)]
enum MixerOp {
    ShutDown,
    SetIterator(ArcIt),
}

pub struct Mixer {
    control_channel : Sender<MixerOp>,
}

impl Mixer {
    fn new(samples : Arc<Vec<i8>>, freq : Freq, out_buf : Arc<Mutex<OutputBuffer>>, callback_vtsensor : TrackerSensor) -> Mixer {
	let (tx, rx) = mpsc::channel();

	let _ = thread::spawn(move || {
	    run_mixer_thread(freq, samples, out_buf.clone(), rx, callback_vtsensor);
	});

	return Mixer {
	    control_channel : tx,
	}
    }
    pub fn set_iterator(&mut self, it : ArcIt) {
	self.control_channel.send(MixerOp::SetIterator(it)).unwrap();
    }
}

// ================================================================================
// MixerThread

struct MixerThread {
    pipeline : LinearFilteringPipeline,
    control_channel : Receiver<MixerOp>,
    buf : Arc<Mutex<OutputBuffer>>,
    tmp_buf : OutputBuffer,
    trackers : Vec<RefCell<Tracker>>,
}

fn run_mixer_thread(freq : Freq,
		    samples : Arc<Vec<i8>>,
		    buf : Arc<Mutex<OutputBuffer>>,
		    control_channel : Receiver<MixerOp>,
		    callback_vtsensor : TrackerSensor)
{
    let sample_source = Rc::new(SimpleSampleSource::from_iter(samples.iter()));
    let tracker_stereo = Tracker::new("Stereo".to_string());
    let tracker_aqueue = Tracker::new("AudioQueue".to_string());
    let tracker_linear = Tracker::new("LinearFilter".to_string());
    let mut tracker_callback = Tracker::new("Callback".to_string());
    tracker_callback.replace_tracker(callback_vtsensor);
    let pipeline = LinearFilteringPipeline::new(iterator::silent(), sample_source, freq,
						tracker_aqueue.sensor(), tracker_linear.sensor(), tracker_stereo.sensor());

    // let sample_source = Rc::new(SimpleSampleSource::new(vec![-80, 80]));
    // let pipeline = LinearFilteringPipeline::new(iterator::simple(vec![AQOp::SetFreq(1000),
    // 								      AQOp::SetSamples(vec![AQSample::Loop(SampleRange::new(0, 2))]),
    // 								      AQOp::WaitMillis(10000)]), sample_source, freq);

    let mut mt = MixerThread {
	pipeline,
	control_channel,
	buf,
	tmp_buf : OutputBuffer::new(),
	trackers : vec![RefCell::new(tracker_aqueue), RefCell::new(tracker_linear), RefCell::new(tracker_stereo), RefCell::new(tracker_callback)],
    };
    mt.run();
}

impl MixerThread {
    fn run(&mut self) {
	loop{
	    if self.check_messages() {
		break;
	    }

	    let samples_needed = self.check_samples_needed();
	    let samples_available = self.check_samples_available();
	    let samples_missing = if samples_needed < samples_available {0} else {samples_needed - samples_available};
	    let samples_to_request = self.fill_heuristic(samples_available, samples_needed, samples_missing);

	    debug!("[AudioThread] fill: {samples_available}/{samples_needed}; expect {samples_needed} -> requesting {samples_to_request}");
	    if log_enabled!(Level::Info) {
		for tracker in self.trackers.iter() {
		    info!("Volume {}", tracker.borrow());
		}
	    }
	    for tracker in self.trackers.iter() {
		tracker.borrow_mut().shift();
	    }
	    if samples_to_request > 0 {
		self.fill_samples(samples_to_request);
	    }

	    self.fill_buffer();

	    // Done for now, wait
	    thread::sleep(time::Duration::from_millis(30));
	}
    }

    fn check_messages(&mut self) -> bool {
	for op in self.control_channel.try_iter() {
	    match op {
		MixerOp::ShutDown        => { return true },
		MixerOp::SetIterator(it) => { self.pipeline.set_iterator(it) },
	    }
	};
	return false;
    }

    fn check_samples_needed(&mut self) -> usize {
	let guard = self.buf.lock().unwrap();
	return guard.expected_read();
    }

    fn check_samples_available(&mut self) -> usize {
	let sdlbuf_available = {let guard = self.buf.lock().unwrap();
				guard.len() };
	return sdlbuf_available + self.tmp_buf.len();
    }

    // Decide how much to add
    fn fill_heuristic(&self, currently_available : usize, average_read : usize, current_needed : usize) -> usize {
	const PROVISION_FACTOR : usize = 1;

	let desired = average_read * (PROVISION_FACTOR + 1);

	if current_needed == 0 && currently_available >= average_read * PROVISION_FACTOR {
	    // twice as many as needed: we're good
	    return 0;
	}
	return (desired - currently_available + 1) & !1; // always even
    }

    // Run the pipeline
    fn fill_samples(&mut self, samples_to_pull : usize) {
	const FILL_BUFFER_SIZE : usize = 64;

	let mut inner_buf : [f32; FILL_BUFFER_SIZE] = [0.0; FILL_BUFFER_SIZE];

	let mut samples_transferred = 0;
	while samples_transferred < samples_to_pull {
	    if samples_transferred > 0 {
		inner_buf.fill(0.0);
	    }
	    self.pipeline.stereo_mapper.borrow_mut().write_stereo_pcm(&mut inner_buf);
	    samples_transferred += FILL_BUFFER_SIZE;
	    self.tmp_buf.read_from(&inner_buf);
	}
    }

    // Write to the output buffer
    fn fill_buffer(&mut self) {
	const FILL_BUFFER_SIZE : usize = 64;

	let mut inner_buf : [f32; FILL_BUFFER_SIZE] = [0.0; FILL_BUFFER_SIZE];

	let mut guard = self.buf.lock().unwrap();
	let outbuf = guard.deref_mut();

	let mut samples_transferred = 0;
	while !outbuf.is_full() && !self.tmp_buf.is_empty() {
	    if samples_transferred > 0 {
		inner_buf.fill(0.0);
	    }
	    let max_transfer = usize::min(FILL_BUFFER_SIZE,
					  outbuf.remaining_capacity());
	    let read_samples = self.tmp_buf.write_to(&mut inner_buf[0..max_transfer]);
	    samples_transferred += outbuf.read_from(&inner_buf[0..read_samples]);
	}
    }
}

// ================================================================================
// Test / demo functionality

pub fn make_note(f : Freq, aqop : AQOp, millis : usize) -> ArcIt{
    let mut ops = vec![AQOp::SetFreq(f), aqop, AQOp::WaitMillis(millis)];

    return iterator::simple(ops);
}
