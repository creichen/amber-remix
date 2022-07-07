use core::fmt;
/// (Most of) the amber music specific bits

extern crate lazy_static;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use crate::datafiles::music::BasicSample;
use crate::datafiles::music::Instrument;
use crate::datafiles::music::InstrumentOp;
use crate::datafiles::music::MPOp;
use crate::datafiles::music::MPNote;
use crate::datafiles::music::MPTimbre;
use crate::datafiles::music::Monopattern;
use crate::datafiles::music::SlidingSample;
use crate::datafiles::music::Song;
use crate::datafiles::music::Timbre;
use crate::datafiles::music::Vibrato;
use crate::datafiles::music::VolumeEnvelope;
use crate::datafiles::music::VolumeSpec;
use super::AQSample;
use super::ArcIt;
use super::Freq;
use super::SampleRange;
use super::iterator::AQOp;
use super::iterator::AudioIterator;

// ================================================================================
// Time

const TICK_DURATION_MILLIS : usize = 20;

type Ticks = usize;

// ================================================================================
// Frequencies

type Note = usize;
type APeriod = usize;

// CoSo period values
pub const PERIODS : [APeriod; 7 * 12] = [
    1712 , 1616 , 1524 , 1440 , 1356 , 1280 , 1208 , 1140 , 1076 , 1016 ,   960 ,   906,
    856  ,  808 ,  762 ,  720 ,  678 ,  640 ,  604 ,  570 ,  538 ,  508 ,   480 ,   453,
    428  ,  404 ,  381 ,  360 ,  339 ,  320 ,  302 ,  285 ,  269 ,  254 ,   240 ,   226,
    214  ,  202 ,  190 ,  180 ,  170 ,  160 ,  151 ,  143 ,  135 ,  127 ,   120 ,   113,
    113  ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,   113 ,   113,
    3424 , 3232 , 3048 , 2880 , 2712 , 2560 , 2416 , 2280 , 2152 , 2032 ,  1920 ,  1812,
    6848 , 6464 , 6096 , 5760 , 5424 , 5120 , 4832 , 4560 , 4304 , 4064 ,  3840 ,  3624];

pub fn period_to_freq(period : APeriod) -> Freq {
    return (3546894.6 / period as f32) as Freq;
}

pub fn note_to_period(note : Note) -> APeriod {
    return PERIODS[note % PERIODS.len()];
}

// ================================================================================
// Volume

type AVolume = u8;

pub fn volume(avol : AVolume) -> f32 {
    if avol > 63 {
	1.0
    } else {
	(1.0 * avol as f32) / 64.0
    }
}

// ================================================================================
// Instrument iterator

struct InstrumentIterator {
    base_note : InstrumentNote,

    remaining_ticks : Option<usize>, // in case we can't wait all at once
    sample : IISample, // Active sample

    queue : VecDeque<InstrumentOp>,
}

#[derive(Clone, Eq, PartialEq)]
struct Slider {
    bounds : SampleRange,
    current : SampleRange,
    delta : isize,
    ticks_remaining : usize, // None -> loop forever
    ticks_delay : usize,
}

impl Slider {
    pub fn shift(&mut self) {
	self.current.start = self.next_pos();
	if !self.can_move() {
	    // done moving
	    self.delta = 0;
	}
    }

    pub fn can_move(&self) -> bool {
	return self.next_pos() != self.current.start;
    }

    pub fn next_pos(&self) -> usize {
	let max_pos = self.bounds.start + self.bounds.len - self.current.len;
	let new_pos = isize::min(max_pos as isize,
				 isize::max(self.bounds.start as isize,
					    self.current.start as isize + self.delta));
	return new_pos as usize;
    }

    pub fn aqop(&self, first_time : bool) -> AQOp {
	if first_time {
	    return AQOp::SetSamples(vec![AQSample::Loop(self.current)]);
	} else {
	    return AQOp::SetSamples(vec![AQSample::OnceAtOffset(self.current, None), AQSample::Loop(self.current)]);
	}
    }

    pub fn tick(&mut self) -> Option<AQOp> {
	if self.delta == 0 {
	    return None;
	}
	if self.ticks_delay == 0 {
	    self.ticks_delay = self.ticks_remaining;
	    self.shift();
	    return Some(self.aqop(false));
	} else {
	    self.ticks_delay -= 1;
	}
	return None;
    }
}

impl fmt::Display for Slider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slider[{} + {}  {}/{} ticks within {}]",
	       self.current, self.delta, self.ticks_remaining, self.ticks_delay, self.bounds)
    }
}

impl From<SlidingSample> for Slider {
    fn from(s : SlidingSample) -> Self {
	Slider {
	    bounds : s.bounds,
	    current : s.subsample_start,
	    delta : s.delta,
	    ticks_delay : s.delay_ticks,
	    ticks_remaining : s.delay_ticks + 1, // since we immediately decrement again
	}
    }
}

#[derive(PartialEq, Eq)]
enum IISample {
    None,
    Basic(BasicSample),
    Slider(Slider),
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
enum InstrumentNote {
    Relative(Note),
    Absolute(Note),
}

impl InstrumentNote {
    pub fn get(&self) -> Note {
	match self {
	    InstrumentNote::Relative(n) => *n,
	    InstrumentNote::Absolute(n) => *n,
	}
    }

    pub fn to_period(&self) -> APeriod {
	note_to_period(self.get())
    }

    pub fn modify(&mut self, change : isize) {
	match self {
	    InstrumentNote::Relative(n) => *n = ((*n as isize + change) & 0x7f) as usize,
	    InstrumentNote::Absolute(_) => {}, // can't modify absolute notes
	}
    }
}

impl InstrumentIterator {
    pub fn new(ops : &Vec<InstrumentOp>, base_note : Note) -> InstrumentIterator {
	let ops2 = (&ops[..]).to_vec();
	let queue = VecDeque::from(ops2);
	InstrumentIterator {
	    base_note : InstrumentNote::Relative(base_note),

	    remaining_ticks : Some(0),
	    sample : IISample::None,

	    queue,
	}
    }

    pub fn simple(ops : &Vec<InstrumentOp>) -> InstrumentIterator {
	InstrumentIterator::new(ops, 0)
    }

    pub fn default() -> InstrumentIterator {
	let v = vec![];
	InstrumentIterator::simple(&v)
    }

    /// May push sample changes
    fn process_queue(&mut self,
		     reset_volume: &mut bool,
		     out_queue: &mut VecDeque<AQOp>) {
        match self.queue.pop_front() {
	    Some(InstrumentOp::WaitTicks(t)) => {
		self.remaining_ticks = Some(t);
	    },
	    Some(InstrumentOp::Loop(v)) => {
		let v2 = (&v[..]).to_vec();
		self.queue = VecDeque::from(v2);
		self.queue.push_back(InstrumentOp::Loop(v));
	    },
	    Some(InstrumentOp::StopSample) => {
		self.sample = IISample::None;
	    },
	    Some(InstrumentOp::Sample(basicsample)) => {
		if IISample::Basic(basicsample) != self.sample {
		    self.sample = IISample::Basic(basicsample);
		    out_queue.push_back(AQOp::from(basicsample));
		}
	    },
	    Some(InstrumentOp::Slide(slidingsample)) => {
		let slider = &Slider::from(slidingsample);
		self.sample = IISample::Slider(slider.clone());
		out_queue.push_back(AQOp::from(slider.aqop(true)));
	    },
	    Some(InstrumentOp::ResetVolume) => {
		*reset_volume = true;
	    },
	    Some(InstrumentOp::Pitch(p)) => {
		self.base_note = InstrumentNote::Relative(p as usize);
	    },
	    Some(InstrumentOp::FixedNote(nnote)) => {
		self.base_note = InstrumentNote::Absolute(nnote as usize);
	    },
	    Some(op) => { warn!("Ignoring {op}") },
	    None     => {
		info!("Finished playing instrument");
		self.remaining_ticks = None;
	    },
	}
    }

    /// May push sample changes
    /// May reset the timbre envelope
    pub fn tick(&mut self,
		_channel_state : &mut ChannelState,
		timbre_iterator : &mut TimbreIterator,
		out_queue : &mut std::collections::VecDeque<AQOp>) {

	match self.remaining_ticks {
	    Some(0) => {},
	    Some(n) => { self.remaining_ticks = Some(n-1);
			 return; },
	    None    => return, // Wait forever
	}

	let mut reset_volume = false;

	while Some(0) == self.remaining_ticks {
	    self.process_queue(&mut reset_volume,
			       out_queue)
	}

	if reset_volume {
	    timbre_iterator.reset_volume();
	}

	// Slider sample handling
	match &self.sample {
	    IISample::Slider(slider) => {
		let mut newslider = slider.clone();
		if let Some(update) = &newslider.tick() {
		    out_queue.push_back(update.clone());
		}
		self.sample = IISample::Slider(newslider.clone());
	    },
	    _ => {},
	}
    }

    /// May set the note
    pub fn tick_note(&mut self,
		     channel_state : &mut ChannelState) {
	match self.base_note {
	    InstrumentNote::Relative(n) => channel_state.note.modify(n as isize),
	    InstrumentNote::Absolute(_) => channel_state.note = self.base_note,
	}
    }
}

// ================================================================================
// Timbre Iterator

struct VibratoState {
    delay : Ticks,

    spec : Vibrato,
    depth : isize,
    direction : isize, // -1 or 1 to indicate which direction we're moving (factor for "slope")
}

impl VibratoState {
    pub fn tick(&mut self){
	if self.spec.slope == 0 {
	    return;
	}

	let max = self.spec.depth;
	let min = -max;

	self.depth += self.direction * self.spec.slope;
	if self.depth <= min {
	    self.depth = min;
	    self.direction *= -1;
	} else if self.depth >= max {
	    self.depth = max;
	    self.direction *= -1;
	}
    }

    pub fn vibrate_period(&self, period : APeriod) -> APeriod {
	return ((period as isize * (1024 + self.depth)) as usize) >> 10;
    }
}

struct TimbreIterator {
    volume_queue    : VecDeque<VolumeSpec>,
    volume_attack   : Vec<VolumeSpec>,
    volume_sustain  : Vec<VolumeSpec>,
    current_avolume : AVolume,

    delay : Option<Ticks>,

    vibrato : VibratoState,
}

impl TimbreIterator {
    pub fn new(timbre : &Timbre) -> TimbreIterator {
	let vq = (&timbre.vol.attack[..]).to_vec();
	let vq2 = (&timbre.vol.attack[..]).to_vec();
	let vsustain = (&timbre.vol.sustain[..]).to_vec();
	TimbreIterator {
	    volume_queue    : VecDeque::from(vq),
	    volume_attack   : vq2,
	    volume_sustain  : vsustain,
	    current_avolume : 0,
	    delay : Some(0),
	    vibrato : VibratoState {
		delay : timbre.vibrato_delay,
		spec : timbre.vibrato,
		depth : timbre.vibrato.depth,
		direction : -1,
	    }
	}
    }

    pub fn default() -> TimbreIterator {
	TimbreIterator::new(&DEFAULT_TIMBRE)
    }

    /// Restart volume envelope, but not Vibrato
    pub fn reset_volume(&mut self) {
	let vq = (&self.volume_attack[..]).to_vec();
	self.volume_queue = VecDeque::from(vq);
	self.delay = Some(0);
    }

    /// Will write volume
    /// NB: This does NOT handle vibrato.  Instead, "tick_vibrato" does.
    pub fn tick(&mut self, state : &mut ChannelState, _out_queue : &mut VecDeque<AQOp>) {
	// Are we ready?
	state.avolume = self.current_avolume;

	match self.delay {
	    None    => return, // indefinite hiatus
	    Some(0) => {},
	    Some(n) => { self.delay = Some(n-1);
			 return; }
	}

	loop {
	    match self.volume_queue.pop_front() {
		Some(vs) => {
		    self.delay = Some(vs.duration);
		    self.current_avolume = vs.volume;
		    state.avolume = vs.volume;
		    break;
		},
		None => {
		    if self.volume_sustain.len() == 0 {
			// We are done
			self.delay = None;
			return;
		    }
		    let vq = (&self.volume_sustain[..]).to_vec();
		    self.volume_queue = VecDeque::from(vq);
		},
	    }
	}
    }

    pub fn tick_vibrato(&mut self, state : &mut ChannelState) {
	if self.vibrato.delay > 0 {
	    self.vibrato.delay -= 1;
	    return;
	}
	self.vibrato.tick();
	state.period = self.vibrato.vibrate_period(state.period);
    }

}

lazy_static! {
    static ref DEFAULT_TIMBRE : Timbre = Timbre{
	envelope_speed : 1,
	instrument : None,
	vibrato : Vibrato { slope : 0, depth : 0 },
	vibrato_delay : 0,
	vol : VolumeEnvelope { attack : vec![VolumeSpec { volume : 64, duration : 1 }], sustain : vec![] }
    };
}

// ================================================================================
// Monopattern Iterator

enum MPStep {
    OK,
    Stop, // Done playing the pattern
    SetTimbre(TimbreIterator, Option<InstrumentIterator>),
}

struct PortandoState {
    delta : isize,
    current : isize,
}

impl PortandoState {
    pub fn empty() -> PortandoState {
	PortandoState { delta : 0,
			current : 0,
	}
    }
    pub fn tick(&mut self) {
	self.current += self.delta;
    }
    pub fn portando(&self, period : APeriod) -> APeriod {
	return ((period as isize * (1024 + self.current)) as usize) >> 10;
    }
}

struct MonopatternIterator {
    portando : PortandoState,
    ops : VecDeque<MPOp>,

    channel_note : isize,

    delay : Option<Ticks>,
}

impl MonopatternIterator {
    pub fn new(ops : &[MPOp]) -> MonopatternIterator {
	let ops = ops.to_vec();
	MonopatternIterator {
	    portando : PortandoState::empty(),
	    ops : VecDeque::from(ops),
	    channel_note : 0,
	    delay : Some(0),
	}
    }

    pub fn default() -> MonopatternIterator {
	let v = vec![MPOp { note : None, pticks : 100000000 } ];
	return MonopatternIterator::new(&v);
    }

    pub fn tick(&mut self,
		state : &mut ChannelState,
		instrument_bank : &Vec<Instrument>,
		timbre_bank : &Vec<Timbre>) -> MPStep {
	match self.delay {
	    None    => return MPStep::Stop, // indefinite hiatus
	    Some(0) => {},
	    Some(n) => { self.delay = Some(n-1);
			 return MPStep::OK; }
	}
	if let Some(n) = self.ops.front() {
	    info!("  Monopattern: play {n}");
	}

	if let Some(MPOp { pticks, note }) = self.ops.pop_front() {
	    self.delay = Some((pticks * state.channel_speed) - 1);
	    match note {
		None => return MPStep::OK,
		Some(MPNote { note, timbre, portando }) => {
		    self.channel_note = note;
		    match portando {
			None        => {
			    if self.portando.current != 0 {
				debug!{"  MP: portando completed"};
			    }
			    self.portando = PortandoState::empty();
			},
			Some(delta) => {
			    debug!{"  MP: portando~{delta}"};
			    self.portando = PortandoState { current : 0, delta };
			},
		    }
		    match timbre {
			None => { return MPStep::OK; },
			Some (MPTimbre { timbre, instrument }) => {
			    let timbre = &timbre_bank[timbre];
			    let instrument = if let Some(instrument_index) = instrument {
				Some(&instrument_bank[instrument_index].ops)
			    } else {
				timbre.instrument.map(|index| &instrument_bank[index as usize].ops)
			    };
			    return MPStep::SetTimbre(TimbreIterator::new(timbre),
						     instrument.map(|instrop| InstrumentIterator::simple(instrop)));
			},
		    }
		}
	    }
	} else {
	    self.delay = None;
	    return MPStep::Stop;
	}
    }

    /// May update state.note
    pub fn tick_note(&mut self, state : &mut ChannelState) {
	let old = state.note.clone();
	state.note.modify(self.channel_note);
	trace!("    MP: note update: {:?} -> {:?}", old, state.note);
    }

    /// May update state.period
    pub fn tick_portando(&mut self, state : &mut ChannelState) {
	self.portando.tick();
	let p2 = self.portando.portando(state.period);
	trace!("    MP: portando: {}, hence {} -> {p2}", self.portando.current, state.period);
	state.period = p2;
    }
}

// ================================================================================
// Channel Iterator
//
// The channel iterator handles all audio for one voice.


struct ChannelState {
    // Persistent state (carried across iterations)
    base_note : Note, // Note requested for manual play
    channel_speed : usize,

    // Transient state (reset every iteration)
    note : InstrumentNote,
    avolume : AVolume,
    period : APeriod,
    num_ticks : Ticks, // Aggregate ticks
}

struct ChannelIterator {
    state : ChannelState,

    instrument_bank : Vec<Instrument>,
    timbre_bank : Vec<Timbre>,

    instrument : InstrumentIterator,
    timbre : TimbreIterator,
    monopattern : MonopatternIterator,
}

impl ChannelIterator {
    fn new(base_note : Note,
	   instrument_bank : Vec<Instrument>,
	   timbre_bank : Vec<Timbre>,
	   instrument : InstrumentIterator,
	   timbre : TimbreIterator,
	   monopattern : MonopatternIterator) -> ChannelIterator {
	ChannelIterator {
	    state : ChannelState {
		base_note,
		note : InstrumentNote::Relative(0),
		channel_speed : 5,

		avolume : 64,
		period : 0,
		num_ticks : 0,
	    },
	    instrument_bank,
	    timbre_bank,
	    instrument,
	    timbre,
	    monopattern,
	}
    }
}

impl AudioIterator for ChannelIterator {
    fn next(&mut self, out_queue : &mut VecDeque<AQOp>) {
	// One full song iterator iteration
	info!("===== Tick #{}", self.state.num_ticks);
	self.state.note = InstrumentNote::Relative(self.state.base_note);
	trace!("  : initial note {:?}", self.state.note);
	let last_period = self.state.period;

	match self.monopattern.tick(&mut self.state, &self.instrument_bank, &self.timbre_bank) {
	    MPStep::OK              => {},
	    MPStep::Stop            => (
		info!("  : Finished Monopattern")
	    ),
	    MPStep::SetTimbre(ti, instr_opt) => {
		info!("  : Timbre/Instrument switch");
		self.timbre = ti;
		if let Some(instr) = instr_opt {
		    self.instrument = instr;
		}
	    }
	}
	self.instrument.tick(&mut self.state, &mut self.timbre, out_queue);
	self.timbre.tick(&mut self.state, out_queue);

	self.instrument.tick_note(&mut self.state);
	self.monopattern.tick_note(&mut self.state);

	let note = self.state.note;

	// Compute the Amiga "period", which then translates to the frequency
	self.state.period = note.to_period();
	self.timbre.tick_vibrato(&mut self.state);
	self.monopattern.tick_portando(&mut self.state);

	// Done with updating, send updates downstream
	// Send updates downstream
	if last_period != self.state.period {
	    out_queue.push_back(AQOp::SetFreq(period_to_freq((self.state.period) as Note)));
	}
	out_queue.push_back(AQOp::SetVolume(volume(self.state.avolume)));
	out_queue.push_back(AQOp::WaitMillis(TICK_DURATION_MILLIS));
	info!("   : note={:?}, period={}", note, self.state.period);
	info!("   :: {:?}", out_queue);
	self.state.num_ticks += 1;
    }
}


pub fn play_timbre(song : &Song, instr : &Instrument, timbre : &Timbre, note : Note) -> ArcIt {
    let instrument = match timbre.instrument {
	None    => instr,
	Some(n) => &song.instruments[n as usize],
    };
    return Arc::new(Mutex::new(ChannelIterator::new(note,
						    (&song.instruments[..]).to_vec(),
						    (&song.timbres[..]).to_vec(),
						    InstrumentIterator::new(&instrument.ops, note),
						    TimbreIterator::new(&timbre),
						    MonopatternIterator::default())));
}

pub fn play_instrument(instr : &Instrument, note : Note) -> ArcIt {
    return Arc::new(Mutex::new(ChannelIterator::new(note,
						    vec![],
						    vec![],
						    InstrumentIterator::new(&instr.ops, note),
						    TimbreIterator::default(),
						    MonopatternIterator::default())));
}

pub fn play_monopattern(song : &Song, pat : &Monopattern, note : Note) -> ArcIt {
    return Arc::new(Mutex::new(ChannelIterator::new(note,
						    (&song.instruments[..]).to_vec(),
						    (&song.timbres[..]).to_vec(),
						    InstrumentIterator::default(),
						    TimbreIterator::default(),
						    MonopatternIterator::new(&pat.ops))));
}
