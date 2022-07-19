// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use core::time;
use std::{sync::{Arc, Mutex, mpsc::{self, Sender, Receiver}}, thread, rc::Rc, cell::RefCell, process};
use std::panic;
use std::ops::DerefMut;
use sdl2::audio::{AudioSpec, AudioCallback, AudioFormat};

use crate::audio::dsp::{crossfade_linear::LinearCrossfade, writer::RcSyncWriter};

use self::{queue::AudioIteratorProcessor, samplesource::RcSampleSource, dsp::{ringbuf::RingBuf, vtracker::{TrackerSensor, Tracker}, writer::RcSyncBarrier, pcmsync}, iterator::ArcPoly, iterator_sequencer::IteratorSequencer};
#[allow(unused)]
use self::{dsp::{linear::LinearFilter, stereo_mapper::StereoMapper, writer::PCMFlexWriter}, queue::AudioQueue, samplesource::SimpleSampleSource, samplesource::SincSampleSource};
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
mod iterator_sequencer;
mod samplesource;
pub mod amber;

const AUDIO_BUF_MAX_SIZE : usize = 16384;

// ================================================================================
// Filtering Pipeline

trait AudioPipeline {
    // Update audio iterator
    fn set_iterator(&mut self, it : ArcIt);
    fn fill_stereo_output(&mut self, out : &mut [f32]);
}

type RcAudioPipeline = Rc<RefCell<dyn AudioPipeline>>;

// ----------------------------------------

struct LinearFilteringPipeline {
    it_proc : Rc<RefCell<dyn AudioIteratorProcessor>>,
    // aqueue : Rc<RefCell<dyn PCMFlexWriter>>,
    // linear_filter : Rc<RefCell<LinearFilter>>,
    stereo_mapper : Rc<RefCell<StereoMapper>>,
}

impl LinearFilteringPipeline {
    fn new(it : ArcIt, vol_left : f32, vol_right : f32,
	   sample_source : RcSampleSource, output_freq : Freq,
	   sync : RcSyncBarrier,
	   sen_queue : TrackerSensor, sen_linear : TrackerSensor, sen_stereo : TrackerSensor) -> RcAudioPipeline {
	let aqueue = Rc::new(RefCell::new(AudioQueue::new(it, sample_source, sen_queue)));
	let unsynced_linear_filter = Rc::new(RefCell::new(LinearFilter::new(40000, output_freq, aqueue.clone(), sen_linear)));
	let linear_filter = sync.borrow_mut().sync(unsynced_linear_filter);
	let stereo_mapper = Rc::new(RefCell::new({let mut s = StereoMapper::new(1.0, 1.0, linear_filter.clone(), sen_stereo);
						  s.set_volume(vol_left, vol_right);
						  s}));
	return Rc::new(RefCell::new(LinearFilteringPipeline {
	    it_proc : aqueue.clone(),
	    // aqueue,
	    // linear_filter,
	    stereo_mapper,
	}));
    }
}

impl AudioPipeline for LinearFilteringPipeline {
    fn set_iterator(&mut self, it : ArcIt) {
	self.it_proc.borrow_mut().set_source(it);
    }

    fn fill_stereo_output(&mut self, out : &mut [f32]) {
        self.stereo_mapper.borrow_mut().write_stereo_pcm(out);
    }
}
// ----------------------------------------

struct SincPipeline {
    it_proc : Rc<RefCell<dyn AudioIteratorProcessor>>,
    stereo_mapper : Rc<RefCell<StereoMapper>>,
}

impl SincPipeline {
    fn new(it : ArcIt, vol_left : f32, vol_right : f32,
	   samples : Rc<RefCell<SincSampleSource>>,
	   target_freq : Freq,
	   sync : RcSyncBarrier,
	   sen_queue : TrackerSensor, sen_linear : TrackerSensor, sen_stereo : TrackerSensor) -> RcAudioPipeline {
	const LINEAR_FILTER : usize = 4;
	let itseq_base = Rc::new(RefCell::new(IteratorSequencer::new_with_source(it, target_freq, samples.clone(), sen_queue)));
	let itseq : RcSyncWriter = if LINEAR_FILTER == 0 { itseq_base.clone() } else {
	    LinearCrossfade::new_rc(LINEAR_FILTER, itseq_base.clone())
	};
	let itseq = sync.borrow_mut().sync(itseq.clone());
	let stereo_mapper = Rc::new(RefCell::new({let mut s = StereoMapper::new(1.0, 1.0, itseq.clone(), sen_stereo);
						  s.set_volume(vol_left, vol_right);
						  s}));
	return Rc::new(RefCell::new(SincPipeline {
	    it_proc : itseq_base.clone(),
	    stereo_mapper,
	}));
    }
}

impl AudioPipeline for SincPipeline {
    fn set_iterator(&mut self, it : ArcIt) {
	self.it_proc.borrow_mut().set_source(it);
    }

    fn fill_stereo_output(&mut self, out : &mut [f32]) {
        self.stereo_mapper.borrow_mut().write_stereo_pcm(out);
    }
}


// ================================================================================
// Callback

struct Callback {
    shared_buf : Arc<Mutex<RingBuf>>,
    tracker : TrackerSensor,
}

impl AudioCallback for Callback {
    type Channel = f32;

    fn callback(&mut self, output: &mut [Self::Channel]) {
	let mut guard = self.shared_buf.lock().unwrap();
	let buf = guard.deref_mut();
	let num_written = buf.write_to(output);

	if num_written < output.len() {
	    println!("Buffer underrun {num_written}/{}", output.len());
	    //warn!("Buffer underrun {num_written}/{}", output.len());
	}
	for x in output[num_written..].iter_mut() {
	    *x = 0.0;
	}
	let mut debug_total = 0.0;
	for x in output[0..num_written].iter() {
	    debug_total += f32::abs(*x);
	}
	self.tracker.add_many(debug_total, output.len());
    }
}

impl Callback {
    fn new(shared_buf : Arc<Mutex<RingBuf>>, tracker : TrackerSensor) -> Callback {
	return Callback {
	    shared_buf, tracker,
	}
    }
}

// ================================================================================
// AudioCore

pub struct AudioCore {
    spec : AudioSpec,
    shared_buf : Arc<Mutex<RingBuf>>,
    device : Option<sdl2::audio::AudioDevice<Callback>>,
    callback_tracker_sensor : TrackerSensor,
}

impl AudioCore {
    fn init(&mut self, spec : AudioSpec) -> Callback {
	self.spec = spec;
	return Callback::new(self.shared_buf.clone(), self.callback_tracker_sensor.clone());
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
	shared_buf : Arc::new(Mutex::new(RingBuf::new(AUDIO_BUF_MAX_SIZE))),
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
    SetPoly(ArcPoly),
}

pub struct Mixer {
    control_channel : Sender<MixerOp>,
}

impl Mixer {
    fn new(samples : Arc<Vec<i8>>, freq : Freq, out_buf : Arc<Mutex<RingBuf>>, callback_vtsensor : TrackerSensor) -> Mixer {
	let (tx, rx) = mpsc::channel();

	let stacktrace_hook = panic::take_hook();
	let _ = thread::spawn(move || {
	    // Completely bail on panic
	    panic::set_hook(Box::new(move |panic_info| {
		stacktrace_hook(panic_info);
		process::exit(1);
	    }));
	    run_mixer_thread(freq, samples, out_buf.clone(), rx, callback_vtsensor);
	});

	return Mixer {
	    control_channel : tx,
	}
    }

    pub fn set_iterator(&mut self, it : ArcIt) {
	self.control_channel.send(MixerOp::SetIterator(it)).unwrap();
    }

    pub fn set_polyiterator(&mut self, it : ArcPoly) {
	self.control_channel.send(MixerOp::SetPoly(it)).unwrap();
    }

    pub fn shutdown(&mut self) {
	self.control_channel.send(MixerOp::ShutDown).unwrap();
    }
}

// ================================================================================
// MixerThread

struct MixerThread {
    amiga_pipelines : [RcAudioPipeline; 4],
    // aux_pipeline : LinearFilteringPipeline,
    control_channel : Receiver<MixerOp>,
    buf : Arc<Mutex<RingBuf>>,
    tmp_buf : RingBuf,
    trackers : Vec<RefCell<Tracker>>,
}

fn make_linear_pipeline(name : &str,
		 left : f32, right : f32,
		 sample_source : RcSampleSource,
		 freq : Freq,
		 sync : RcSyncBarrier,
		 trackers : &mut Vec<RefCell<Tracker>>
                 ) -> RcAudioPipeline {
    let tracker_aqueue = Tracker::new(format!("{name}:AQueue"));
    let tracker_linear = Tracker::new(format!("{name}:LFiltr"));
    let tracker_stereo = Tracker::new(format!("{name}:Stereo"));

    let pipeline = LinearFilteringPipeline::new(iterator::silent(),
						left, right,
						sample_source, freq,
						sync,
						tracker_aqueue.sensor(), tracker_linear.sensor(), tracker_stereo.sensor());

    trackers.push(RefCell::new(tracker_aqueue));
    trackers.push(RefCell::new(tracker_linear));
    trackers.push(RefCell::new(tracker_stereo));
    return pipeline;
}

fn make_sinc_pipeline(name : &str,
		 left : f32, right : f32,
		 sample_source : Rc<RefCell<SincSampleSource>>,
		 freq : Freq,
		 sync : RcSyncBarrier,
		 trackers : &mut Vec<RefCell<Tracker>>
                 ) -> RcAudioPipeline {
    let tracker_aqueue = Tracker::new(format!("{name}:AQueue"));
    let tracker_linear = Tracker::new(format!("{name}:LFiltr"));
    let tracker_stereo = Tracker::new(format!("{name}:Stereo"));

    let pipeline = SincPipeline::new(iterator::silent(),
				     left, right,
				     sample_source, freq,
				     sync,
				     tracker_aqueue.sensor(), tracker_linear.sensor(), tracker_stereo.sensor());

    trackers.push(RefCell::new(tracker_aqueue));
    trackers.push(RefCell::new(tracker_linear));
    trackers.push(RefCell::new(tracker_stereo));
    return pipeline;
}

fn run_mixer_thread(freq : Freq,
		    samples : Arc<Vec<i8>>,
		    buf : Arc<Mutex<RingBuf>>,
		    control_channel : Receiver<MixerOp>,
		    callback_vtsensor : TrackerSensor) {
    let tracker_stereo = Tracker::new("Stereo".to_string());
    let tracker_aqueue = Tracker::new("AudioQueue".to_string());
    let tracker_linear = Tracker::new("LinearFilter".to_string());

    let sync_amiga = pcmsync::new_basic();
    let mut trackers = vec![RefCell::new(tracker_aqueue), RefCell::new(tracker_linear), RefCell::new(tracker_stereo)];

    let amiga_pipelines : [RcAudioPipeline; 4] = if false {
	let sample_source = Rc::new(RefCell::new(SimpleSampleSource::from_iter(samples.iter())));
	[
	    make_linear_pipeline("A0L", 0.5, 0.0, sample_source.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_linear_pipeline("A1R", 0.0, 0.5, sample_source.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_linear_pipeline("A2R", 0.0, 0.5, sample_source.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_linear_pipeline("A3L", 0.5, 0.0, sample_source.clone(), freq, sync_amiga.clone(), &mut trackers),
	]
    } else {
	let samples = Rc::new(RefCell::new(SincSampleSource::from_i8(freq, samples.as_ref())));
	[
	    make_sinc_pipeline("A0L", 0.5, 0.0, samples.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_sinc_pipeline("A1R", 0.0, 0.5, samples.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_sinc_pipeline("A2R", 0.0, 0.5, samples.clone(), freq, sync_amiga.clone(), &mut trackers),
	    make_sinc_pipeline("A3L", 0.5, 0.0, samples.clone(), freq, sync_amiga.clone(), &mut trackers),
	]
    };

    //

    // let sync_pipeline = pcmsync::new_basic();

    // let pipeline = LinearFilteringPipeline::new(iterator::silent(),
    // 						1.0, 1.0,
    // 						sample_source.clone(), freq,
    // 						sync_pipeline,
    // 						tracker_aqueue.sensor(), tracker_linear.sensor(), tracker_stereo.sensor());


    // let amiga_pipelines = [
    // ];

    let mut tracker_callback = Tracker::new("Callback".to_string());
    tracker_callback.replace_tracker(callback_vtsensor);
    trackers.push(RefCell::new(tracker_callback));

    let mut mt = MixerThread {
	amiga_pipelines,
	// aux_pipeline: pipeline,
	control_channel,
	buf,
	tmp_buf : RingBuf::new(AUDIO_BUF_MAX_SIZE),
	trackers,
    };
    mt.run();
}

const MIXER_THREAD_FREQUENCY_MILLIS : u64 = 20;
const MIXER_SCOPE_OUTPUT_FREQUENCY_MILLIS : u64 = 50; // once per this many milliseconds
const MIXER_OVERPROVISION_FACTOR : f32 = 0.0; // increase prebuffering

impl MixerThread {
    fn run(&mut self) {
	let mut millis_since_last_info_dump = 0;
	let mut fill_states_since_last_info_dump = 0;
	let mut checks_since_last_info_dump = 0;
	loop{
	    if self.check_messages() {
		break;
	    }

	    let samples_needed = self.check_samples_needed();
	    let samples_available = self.check_samples_available();
	    let samples_missing = if samples_needed < samples_available {0} else {samples_needed - samples_available};
	    let samples_to_request = self.fill_heuristic(samples_available, samples_needed, samples_missing);

	    checks_since_last_info_dump += 1;
	    fill_states_since_last_info_dump += samples_available;

	    debug!("[AudioThread] fill: {samples_available}/{samples_needed}; expect {samples_needed} -> requesting {samples_to_request}");
	    if log_enabled!(Level::Info) {
		millis_since_last_info_dump += MIXER_THREAD_FREQUENCY_MILLIS;
		if millis_since_last_info_dump > MIXER_SCOPE_OUTPUT_FREQUENCY_MILLIS {
		    millis_since_last_info_dump -= MIXER_SCOPE_OUTPUT_FREQUENCY_MILLIS;
		    for tracker in self.trackers.iter() {
			info!("Volume {}", tracker.borrow());
		    }
		    info!("[AudioThread] buffer fill: {}/{samples_needed}",
			  fill_states_since_last_info_dump / checks_since_last_info_dump);
		    fill_states_since_last_info_dump = 0;
		    checks_since_last_info_dump = 0;
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
	    thread::sleep(time::Duration::from_millis(MIXER_THREAD_FREQUENCY_MILLIS));
	}
    }

    fn check_messages(&mut self) -> bool {
	for op in self.control_channel.try_iter() {
	    match op {
		MixerOp::ShutDown        => { return true },
		/* mono pipeline temporarily disabled: */
		MixerOp::SetIterator(_it) => { /*self.aux_pipeline.set_iterator(it) */},
		MixerOp::SetPoly(polyit) => {
		    let its : Vec<ArcIt>;
		    {
			let mut guard = polyit.lock().unwrap();
			let p = guard.deref_mut();
			its = p.get();
		    }
		    let mut i = 0;
		    for p in &mut self.amiga_pipelines {
			p.borrow_mut().set_iterator(its[i].clone());
			i += 1;
		    }
		}
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

	let overprovision_desired = (average_read as f32 * MIXER_OVERPROVISION_FACTOR) as usize;

	if current_needed == 0 && currently_available >= average_read + overprovision_desired {
	    // twice as many as needed: we're good
	    return 0;
	}

	return (usize::max(overprovision_desired - currently_available,
			   current_needed) + 1) & !1; // always even
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
	    // Run auxiliary pipeline
	    // self.aux_pipeline.stereo_mapper.borrow_mut().write_stereo_pcm(&mut inner_buf);
	    // Run the four Amiga pipelines
	    for pipeline in &self.amiga_pipelines {
		pipeline.borrow_mut().fill_stereo_output(&mut inner_buf);
	    }
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
    let ops = vec![AQOp::SetFreq(f), aqop, AQOp::WaitMillis(millis)];

    return iterator::simple(ops);
}
