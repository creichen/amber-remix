use core::fmt;
/// (Most of) the amber music specific bits

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use crate::datafiles::music::BasicSample;
use crate::datafiles::music::Instrument;
use crate::datafiles::music::InstrumentOp;
use crate::datafiles::music::SlidingSample;
use super::AQSample;
use super::ArcIt;
use super::Freq;
use super::SampleRange;
use super::iterator::AQOp;
use super::iterator::AudioIterator;

// ================================================================================
// Time

const TICK_DURATION_MILLIS : usize = 20;

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

pub fn note_to_freq(note : Note) -> Freq {
    return period_to_freq(note_to_period(note));
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
    init : bool, // Initial set of specs written

    pitch : isize,
    base_note : Note,
    base_avolume : AVolume,

    remaining_ticks : usize, // in case we can't wait all at once
    period : APeriod,
    sample : IISample, // Active sample

    queue : VecDeque<InstrumentOp>,
}

#[derive(Clone, Eq, PartialEq)]
struct Slider {
    bounds : SampleRange,
    current : SampleRange,
    delta : isize,
    ticks_remaining : usize,
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

impl InstrumentIterator {
    pub fn new(ops : &Vec<InstrumentOp>, base_note : Note, base_avolume : AVolume) -> InstrumentIterator {
	let ops2 = (&ops[..]).to_vec();
	let queue = VecDeque::from(ops2);
	InstrumentIterator {
	    init : false,
	    pitch : 0,
	    base_note,
	    base_avolume,

	    remaining_ticks : 0,
	    period : note_to_period(base_note),
	    sample : IISample::None,

	    queue,
	}
    }
}

impl AudioIterator for InstrumentIterator {
    fn next(&mut self, out_queue : &mut std::collections::VecDeque<AQOp>) {
	// Wrote a Wait into the queue?
	let mut waittime = self.remaining_ticks;
	let mut wrote_freq = false;
	let mut effect = false;

	while waittime == 0 {
	    if self.queue.len() > 0 {
		info!("-- PLAY {}", self.queue[0]);
	    }
	    match self.queue.pop_front() {
		Some(InstrumentOp::WaitTicks(t)) => {
		    waittime = t;
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
		// Some(InstrumentOp::ResetVolume) => {
		// },
		Some(InstrumentOp::Pitch(p)) => {
		    let pitch = p as isize;
		    self.pitch = pitch;
		    wrote_freq = true;
		    out_queue.push_back(AQOp::SetFreq(note_to_freq(((pitch + self.base_note as isize) & 0x7f) as Note)));
		},
		Some(InstrumentOp::FixedNote(note)) => {
		    wrote_freq = true;
		    out_queue.push_back(AQOp::SetFreq(note_to_freq(note as Note)));
		},
		Some(op) => { warn!("Ignoring {op}") },
		None     => {
		    info!("Finished playing instrument");
		    waittime = 10000000;
		},
	    }
	}

	if !self.init {
	    self.init = true;
	    if !wrote_freq {
		out_queue.push_back(AQOp::SetFreq(note_to_freq(self.base_note)));
	    }
	    out_queue.push_back(AQOp::SetVolume(volume(self.base_avolume)));
	}

	match &self.sample {
	    IISample::Slider(slider) => {
		let mut newslider = slider.clone();
		if let Some(update) = &newslider.tick() {
		    out_queue.push_back(update.clone());
		}
		self.sample = IISample::Slider(newslider.clone());
		if newslider.can_move() {
		    effect = true;
		}
	    },
	    _ => {},
	}

	// If an effect is in progress, we do one tick at a time
	if effect && waittime > 0 {
	    self.remaining_ticks = waittime - 1;
	    waittime = 1;
	} else {
	    self.remaining_ticks = 0;
	}

	out_queue.push_back(AQOp::WaitMillis(waittime * TICK_DURATION_MILLIS));
    }
}

pub fn play_instrument(instr : &Instrument, note : Note, avol : AVolume) -> ArcIt {
    return Arc::new(Mutex::new(InstrumentIterator::new(&instr.ops, note, avol)));
}
