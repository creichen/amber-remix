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
    pub start : usize,
    pub len : usize,
}

impl SampleRange {
    fn new(start : usize, len : usize) -> SampleRange {
	SampleRange {
	    start, len,
	}
    }
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
}

#[allow(unused)]
impl AudioCallback for &Mixer {
    type Channel = i16;

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
		    AudioQueueOp::SetVolume(v) => {amplitude = (v * 20000.0) as i16},
		    _ => {},
		}
	    }
	}
        for x in output.iter_mut() {
	    *x = 0;
	}
	mixer_copy_sample(self, output, (32768, 32768), 0x744, 0x2fc2);
    }
}

fn mixer_audio_spec(mixer : &&Mixer) -> AudioSpec {
    let mut guard = mixer.processor.lock().unwrap();
    let proc = guard.deref_mut();
    return proc.audio_spec.unwrap()
}

fn clamp_i16(v : i32) -> i16 {
    return i32::max(-32678, i32::min(32767, v)) as i16;
}

fn mixer_copy_sample(mixer : &&Mixer, outbuf : &mut [i16], volume : (i32, i32), start : usize, end : usize) {
    let mut guard = mixer.processor.lock().unwrap();
    let proc = guard.deref_mut();
    let sample_data = &proc.sample_data;
    let sample = &sample_data[start..end];
    let sample_length = end - start;
    let (vol_l, vol_r) = volume;

    let mut sample_i = 0;

    for out_i in (0..outbuf.len()).step_by(2) {
	let sample_v = sample[sample_i & sample_length] as i32;
	outbuf[out_i] += clamp_i16((sample_v * vol_l) >> 8);
	outbuf[out_i + 1] += clamp_i16((sample_v * vol_r) >> 8);
	sample_i += 1;
    }
}

struct SampleState {
    out_freq : u32,         // output buffer frequency
    sample_freq : u32,

    sample_range : SampleRange,

    // index into sample data
    sample_pos_int : usize,   // integral part
    sample_pos_fract : i32, // fractional part (nominator; the denominator is out_freq)
}

impl SampleState {
    fn new(range : SampleRange, sample_freq : u32, out_freq : u32) -> SampleState {
	SampleState {
	    out_freq : out_freq,
	    sample_freq : sample_freq,
	    sample_range : range,
	    sample_pos_int : 0,
	    sample_pos_fract : 0,
	}
    }

    // // Sample has lower frequency than output buffer
    // fn _mix_into_linear_expanding(&mut self, outbuf : &mut [i16], sample_data : &Vec<i8>, volumes : (i32, i32)) -> bool {
    // 	let (vol_l, vol_r) = volumes;
    // 	let sample_len = self.sample_range.len;
    // 	let sample_buf;
    // 	{
    // 	    let start = self.sample_range.start;
    // 	    sample_buf = &sample_data[start..start+sample_len];
    // 	}

    // 	let mut pos = self.sample_pos_int;

    // 	if pos >= 0 && pos as usize >= sample_len {
    // 	    return true;
    // 	}

    // 	// fractional position counter
    // 	let mut fpos_nom = self.sample_pos_fract;
    // 	let fpos_nom_inc_total = self.sample_freq as i32;
    // 	let fpos_denom = self.out_freq as i32;

    // 	// if fpos_nom_inc_total > fpos_denom, we have to skip at least one sample
    // 	let fpos_nom_inc = fpos_nom_inc_total % fpos_denom;
    // 	let max_skipped_samples : u32 = i32::max(0, (fpos_nom_inc_total / fpos_denom) - 1) as u32;

    // 	for out_i in (0..outbuf.len()).step_by(2) {
    // 	    // Current sample:
    // 	    let sample_v_current = if pos < 0 { sample_buf[0] } else { sample_buf[pos as usize] };

    // 	    let mut sample_v_aggregate : i64 = 0;
    // 	    let sample_v_aggregate_left : i32;
    // 	    let sample_v_aggregate_right : i32;

    // 	    // Aggreagate skipped samples
    // 	    let skipped_samples = u32::max(0, u32::min(max_skipped_samples, (sample_len - pos) as u32)) as usize;
    // 	    if skipped_samples > 0 {
    // 		println!("skipped={skipped_samples}");
    // 		for skip_pos in pos + 1..pos + 1 + skipped_samples {
    // 		    sample_v_aggregate += sample_buf[skip_pos] as i64;
    // 		}
    // 		sample_v_aggregate_left = ((sample_v_aggregate * (vol_l as i64)) / skipped_samples as i64) as i32;
    // 		sample_v_aggregate_right = ((sample_v_aggregate * (vol_r as i64)) / skipped_samples as i64) as i32;
    // 		pos += skipped_samples;
    // 	    } else {
    // 		sample_v_aggregate_left = 0;
    // 		sample_v_aggregate_right = 0;
    // 	    }

    // 	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { sample_buf[pos + 1] };

    // 	    println!("[{pos}] {sample_v_current} .. {sample_v_aggregate_right} .. {sample_v_next}*{fpos_nom}");

    // 	    // Linear interpolation of start and end points
    // 	    let sample_v_current_fragment = sample_v_current as i32 * (fpos_denom - fpos_nom);
    // 	    let sample_v_next_fragment = sample_v_next as i32 * fpos_nom;

    // 	    let sample_v = sample_v_current_fragment + sample_v_next_fragment;
    // 	    let sample_v_left = (sample_v * vol_l) / fpos_denom;
    // 	    let sample_v_right = (sample_v * vol_r) / fpos_denom;

    // 	    outbuf[out_i]     += clamp_i16(sample_v_left  + sample_v_aggregate_left);
    // 	    outbuf[out_i + 1] += clamp_i16(sample_v_right + sample_v_aggregate_right);

    // 	    fpos_nom = fpos_nom + fpos_nom_inc;
    // 	    if fpos_nom >= fpos_denom {
    // 		fpos_nom -= fpos_denom;
    // 		pos += 1;
    // 	    }
    // 	    if pos >= sample_len {
    // 		break;
    // 	    }
    // 	}
    // 	self.sample_pos_int = pos;
    // 	self.sample_pos_fract = fpos_nom;
    // 	return pos >= sample_len;
    // }



    // Sample has lower frequency than output buffer
    fn _mix_into_linear_expanding(&mut self, outbuf : &mut [i16], sample_data : &Vec<i8>, volumes : (i32, i32)) -> bool {
	let (vol_l, vol_r) = volumes;
	let sample_len = self.sample_range.len;
	let sample_buf;
	{
	    let start = self.sample_range.start;
	    sample_buf = &sample_data[start..start+sample_len];
	}

	let mut pos = self.sample_pos_int;

	if pos >= sample_len {
	    return true;
	}

	// fractional position counter
	let mut fpos_nom = self.sample_pos_fract;
	let fpos_nom_inc = self.sample_freq as i32;
	let fpos_denom = self.out_freq as i32;

	for out_i in (0..outbuf.len()).step_by(2) {
	    // Linear interpolation
	    let sample_v_current = sample_buf[pos];

	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { sample_buf[pos + 1] };

	    let sample_v_current_fragment = sample_v_current as i32 * (fpos_denom - fpos_nom);
	    let sample_v_next_fragment = sample_v_next as i32 * fpos_nom;

	    let sample_v = sample_v_current_fragment + sample_v_next_fragment;

	    outbuf[out_i] += clamp_i16((sample_v * vol_l) / fpos_denom);
	    outbuf[out_i + 1] += clamp_i16((sample_v * vol_r) / fpos_denom);

	    fpos_nom = fpos_nom + fpos_nom_inc;
	    if fpos_nom >= fpos_denom {
		fpos_nom -= fpos_denom;
		pos += 1;
	    }
	    if pos >= sample_len {
		break;
	    }
	}
	self.sample_pos_int = pos;
	self.sample_pos_fract = fpos_nom;
	return pos >= sample_len;
    }

    // // Sample has lower frequency than output buffer
    // fn _mix_into_linear_expanding(&mut self, outbuf : &mut [i16], sample_data : &Vec<i8>, volumes : (i32, i32)) -> bool {
    // 	let (vol_l, vol_r) = volumes;
    // 	let sample_len = self.sample_range.len;
    // 	let sample_buf;
    // 	{
    // 	    let start = self.sample_range.start;
    // 	    sample_buf = &sample_data[start..start+sample_len];
    // 	}

    // 	let mut pos = self.sample_pos_int;

    // 	if pos >= sample_len {
    // 	    return true;
    // 	}

    // 	// fractional position counter
    // 	let mut fpos_nom = self.sample_pos_fract;
    // 	let fpos_nom_inc_total = self.sample_freq as i32;
    // 	let fpos_denom = self.out_freq as i32;
    // 	let fpos_nom_inc = fpos_nom_inc_total % fpos_denom;
    // 	let samples_to_skip_total : u32 = i32::max(0, (fpos_nom_inc_total / fpos_denom) - 1) as u32;
    // 	let samples_to_skip_before = (samples_to_skip_total / 2) as usize;
    // 	let samples_to_skip_after = samples_to_skip_total as usize - samples_to_skip_before;
    // 	let mut last_out_i = None;

    // 	pos += samples_to_skip_before;

    // 	for out_i in (0..outbuf.len()).step_by(2) {
    // 	    // Linear interpolation

    // 	    let sample_v_current = sample_buf[pos];

    // 	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { sample_buf[pos + 1] };

    // 	    let sample_v_current_fragment = sample_v_current as i32 * (fpos_denom - fpos_nom);
    // 	    let sample_v_next_fragment = sample_v_next as i32 * fpos_nom;

    // 	    let sample_v = sample_v_current_fragment + sample_v_next_fragment;

    // 	    outbuf[out_i] += clamp_i16((sample_v * vol_l) / fpos_denom);
    // 	    outbuf[out_i + 1] += clamp_i16((sample_v * vol_r) / fpos_denom);

    // 	    fpos_nom = fpos_nom + fpos_nom_inc;
    // 	    if fpos_nom >= fpos_denom {
    // 		fpos_nom -= fpos_denom;
    // 		pos += 1;
    // 	    }
    // 	    pos += samples_to_skip_total as usize;
    // 	    if pos >= sample_len {
    // 		last_out_i = Some(out_i);
    // 		break;
    // 	    }
    // 	}
    // 	self.sample_pos_int = pos;
    // 	self.sample_pos_fract = fpos_nom;
    // 	return pos >= sample_len;
    // }

    // // Sample has higher frequency than output buffer
    // fn _mix_into_linear_contracting(&mut self, outbuf : &mut [i16], sample_data : &Vec<i8>, volumes : (i32, i32)) -> bool {
    // 	let (vol_l, vol_r) = volumes;
    // 	let sample_len = self.sample_range.len;
    // 	let sample_buf;
    // 	{
    // 	    let start = self.sample_range.start;
    // 	    sample_buf = &sample_data[start..start+sample_len];
    // 	}

    // 	let mut pos = self.sample_pos_int;

    // 	if pos >= sample_len {
    // 	    return true;
    // 	}

    // 	// fractional position counter
    // 	let mut fpos_nom = self.sample_pos_fract;
    // 	let fpos_nom_inc = self.sample_freq;
    // 	let fpos_denom = self.out_freq;

    // 	for out_i in (0..outbuf.len()).step_by(2) {
    // 	    // Linear interpolation
    // 	    let sample_v_first = sample_buf[pos];
    // 	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { sample_buf[pos + 1] };

    // 	    let sample_v_current_fragment = sample_v_current as i32 * (fpos_denom - fpos_nom);
    // 	    let sample_v_next_fragment = sample_v_next as i32 * fpos_nom;

    // 	    let sample_v = sample_v_current_fragment + sample_v_next_fragment;

    // 	    outbuf[out_i] += clamp_i16((sample_v * vol_l) / fpos_denom);
    // 	    outbuf[out_i + 1] += clamp_i16((sample_v * vol_r) / fpos_denom);

    // 	    fpos_nom += fpos_nom_inc;
    // 	    if fpos_nom >= fpos_denom {
    // 		fpos_nom -= fpos_denom;
    // 		pos += 1;
    // 	    }
    // 	    if pos >= sample_len {
    // 		break;
    // 	    }
    // 	}
    // 	self.sample_pos_int = pos;
    // 	self.sample_pos_fract = fpos_nom;
    // 	return pos >= sample_len;
    // }

    /// Return true iff the sample was completely copied over
    fn mix_into(&mut self, outbuf : &mut [i16], sample_data : &Vec<i8>, volumes : (i32, i32)) -> bool {
	return if self.sample_freq > self.out_freq {
	    self._mix_into_linear_expanding(outbuf, sample_data, volumes)
	} else {
	    self._mix_into_linear_expanding(outbuf, sample_data, volumes)
	};
    }
}

// ----------------------------------------

#[cfg(test)]
#[test]
fn sample_mix_copy() {
    let mut outbuf : [i16; 16] = [0; 16];
    let inbuf = vec![5, 20, 100, 10, 40];
    let mut sstate = SampleState::new(SampleRange::new(0, 5), 100, 100);
    assert_eq!([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
	       &outbuf[..]);
    let result = sstate.mix_into(&mut outbuf[..], &inbuf, (10, 1));
    assert_eq!( [50,	5,
		 200,	20,
		 1000,	100,
		 100,	10,
		 400,	40,
		 0,	0,
		 0,	0,
		 0,	0],
		 &outbuf[..]);
    assert_eq!(true, result);
}

#[cfg(test)]
#[test]
fn sample_mix_linear_stretch_double() {
    let mut outbuf : [i16; 16] = [0; 16];
    let inbuf = vec![5, 20, 100, 10, 40];
    let mut sstate = SampleState::new(SampleRange::new(0, 5), 4, 8);
    assert_eq!([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
	       &outbuf[..]);
    let result = sstate.mix_into(&mut outbuf[..], &inbuf, (1, 2));
    assert_eq!(false, result);
    assert_eq!( [5,	10,
		 12,	25,
		 20,	40,
		 60,	120,
		 100,	200,
		 55,	110,
		 10,	20,
		 25,	50,
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn sample_mix_linear_stretch_triple() {
    let mut outbuf : [i16; 14] = [0; 14];
    let inbuf = vec![10, 40, 100, 10, 40];
    let mut sstate = SampleState::new(SampleRange::new(0, 5), 4, 12);
    let result = sstate.mix_into(&mut outbuf[..], &inbuf, (1, 1));
    assert_eq!(false, result);
    assert_eq!( [10,	10,
		 20,	20,
		 30,	30,
		 40,	40,
		 60,	60,
		 80,	80,
		 100,	100,
		 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn sample_mix_linear_stretch_interrupted() {
    let mut outbuf : [i16; 24] = [0; 24];
    let inbuf = vec![5, 20, 100, 10, 40];
    let mut sstate = SampleState::new(SampleRange::new(0, 5), 4, 8);
    let result = sstate.mix_into(&mut outbuf[0..2], &inbuf, (1, 2));
    assert_eq!(false, result);
    let result = sstate.mix_into(&mut outbuf[2..6], &inbuf, (1, 2));
    assert_eq!(false, result);
    let result = sstate.mix_into(&mut outbuf[6..], &inbuf, (10, 20));
    assert_eq!(true, result);
    assert_eq!( [5,	10,
		 12,	25,
		 20,	40,
		 600,	1200,
		 1000,	2000,
		 550,	1100,
		 100,	200,
		 250,	500,
		 400,	800,
		 400,	800,
		 0,	0,
		 0,	0,
		 ],
		 &outbuf[..]);
}

// #[cfg(test)]
// #[test]
// fn sample_mix_linear_compact_2() {
//     let mut outbuf : [i16; 8] = [0; 8];
//     let inbuf = vec![50, -10, -10, 100, 20, 30];
//     let mut sstate = SampleState::new(SampleRange::new(0, 5), 8, 4);
//     let result = sstate.mix_into(&mut outbuf[..], &inbuf, (2, 1));
//     assert_eq!( [40,	20,
// 		 90,	45,
// 		 50,	25,
// 		 0,	0,
// 		 ],
// 		 &outbuf[..]);
//     assert_eq!(true, result);
// }

// #[cfg(test)]
// #[test]
// fn sample_mix_linear_compact_3() {
//     let mut outbuf : [i16; 8] = [0; 8];
//     let inbuf = vec![50, -10, -10, 100, 20, 30];
//     let mut sstate = SampleState::new(SampleRange::new(0, 5), 8, 4);
//     let result = sstate.mix_into(&mut outbuf[..], &inbuf, (1, 2));
//     assert_eq!(true, result);
//     assert_eq!( [20,	40,
// 		 75,	150,
// 		 0,	0,
// 		 0,	0,
// 		 ],
// 		 &outbuf[..]);
// }

// #[cfg(test)]
// #[test]
// fn sample_mix_linear_compact_interrupted() {
//     let mut outbuf : [i16; 8] = [0; 8];
//     let inbuf = vec![50, -10, -10, 100, 20, 30];
//     let mut sstate = SampleState::new(SampleRange::new(0, 5), 8, 4);
//     let result = sstate.mix_into(&mut outbuf[0..2], &inbuf, (2, 1));
//     assert_eq!(false, result);
//     let result = sstate.mix_into(&mut outbuf[2..4], &inbuf, (2, 1));
//     assert_eq!(false, result);
//     let result = sstate.mix_into(&mut outbuf[4..], &inbuf, (2, 1));
//     assert_eq!(true, result);
//     assert_eq!( [40,	20,
// 		 90,	45,
// 		 50,	25,
// 		 0,	0,
// 		 ],
// 		 &outbuf[..]);
// }

// ================================================================================

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

