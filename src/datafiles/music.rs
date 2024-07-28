// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use core::fmt;
use std::collections::HashMap;
use crate::{datafiles::decode, audio::SampleRange};

fn fmt_slice<T>(v : &[T]) -> String where T : fmt::Display  {
    let mut s = "".to_string();
    for o in v {
	if s.len() > 0 {
	    s.push_str(" ");
	}
	let str = format!("{}", o);
	s.push_str(&str);
    }
    return s;
}

// ================================================================================
// Samples

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct BasicSample {
    pub attack : SampleRange,            // First sample to play
    pub looping : Option<SampleRange>,   // Then loop over this sample, if present
}

impl fmt::Display for BasicSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	match self.looping {
	    Some(l) => write!(f, "BasicSample[{} +loop:{}]", self.attack, l),
	    None    => write!(f, "BasicSample[{}]", self.attack),
	}
    }
}

/// Multiple samples that we "slide through" while playing
#[derive(Copy, Clone, Debug)]
pub struct SlidingSample {
    pub bounds : SampleRange, // Will stop once it moves into those bounds
    pub subsample_start : SampleRange,
    pub delta : isize,
    pub delay_ticks : usize,
}

// ================================================================================
// Instruments

impl fmt::Display for SlidingSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SlidingSample[{}{}{} / {} ticks; within {}]", self.subsample_start,
	       if self.delta < 0 { "" } else { "+" }, self.delta, self.delay_ticks, self.bounds)
    }
}

#[derive(Clone, Debug)]
pub enum InstrumentOp {
    WaitTicks(usize),        // delay before next step
    Loop(Vec<InstrumentOp>),
    StopSample,              // Force-stop sample
    Sample(BasicSample),     // Change sample; no-op if same sample is still playing
    Slide(SlidingSample),
    ResetVolume,             // Reset timbre volume envelope
    Pitch(i8),               // Relative pitch tweak
    FixedNote(u8),           // Instrument will only play this note
    Unsupported(String),
}

impl fmt::Display for InstrumentOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	match self {
	    InstrumentOp::WaitTicks(ticks) => write!(f, "Wait({ticks})"),
	    InstrumentOp::Loop(vec)        => write!(f, "loop[{}]", fmt_slice(&vec)),
	    InstrumentOp::StopSample       => write!(f, "stopsample"),
	    InstrumentOp::Sample(s)        => write!(f, "{s}"),
	    InstrumentOp::Slide(slider   ) => write!(f, "{slider}"),
	    InstrumentOp::ResetVolume      => write!(f, "reset-vol"),
	    InstrumentOp::Pitch(pitch)     => write!(f, "pitch({pitch})"),
	    InstrumentOp::FixedNote(pitch) => write!(f, "abs-pitch({pitch})"),
	    InstrumentOp::Unsupported(err) => write!(f, "!!UNSUPPORTED({err})!!"),
	}
    }
}

#[derive(Clone)]
pub struct Instrument {
    pub ops : Vec<InstrumentOp>,
}

impl fmt::Display for Instrument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f, "{}", fmt_slice(&self.ops[..]))
    }
}

// ================================================================================
// Timbres

#[derive(Clone, Copy)]
pub struct Vibrato {
    pub slope : isize,
    pub depth : isize,
}

impl fmt::Display for Vibrato {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f, "Vib({} +/- {})", self.depth, self.slope)
    }
}

#[derive(Clone, Copy)]
pub struct VolumeSpec {
    pub volume   : u8,    // 0-64
    pub duration : usize, // ticks to hold before moving on
}

impl fmt::Display for VolumeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f, "{}x{}t", self.volume, self.duration)
    }
}

#[derive(Clone)]
pub struct VolumeEnvelope {
    pub attack   : Vec<VolumeSpec>,
    pub sustain  : Vec<VolumeSpec>,
}

impl fmt::Display for VolumeEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	if self.sustain.len() > 0 {
	    write!(f, "VolEnv [{} | loop: {}]", fmt_slice(&self.attack), fmt_slice(&self.sustain))
	} else {
	    write!(f, "VolEnv [{}]", fmt_slice(&self.attack))
	}
    }
}

#[derive(Clone)]
pub struct Timbre {
    pub envelope_speed : u8, // default ticks per step in the volume envelope
    pub instrument     : Option<u8>, // Default instrument
    pub vibrato        : Vibrato,
    pub vibrato_delay  : usize, // Ticks before vibrato sets in
    pub vol            : VolumeEnvelope,
}

impl fmt::Display for Timbre {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let insn = match self.instrument {
	    None    => "_".to_string(),
	    Some(n) => format!("{:02x}",n),
	};
	write!(f, "I#{insn} {} after {} {}",
	       self.vibrato, self.vibrato_delay, self.vol)
    }
}

// ================================================================================
// Monopatterns

#[derive(Clone, Copy, Debug)]
pub struct MPTimbre {
    pub timbre : usize,
    pub instrument : Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub struct MPNote {
    pub note : isize,
    pub timbre : Option<MPTimbre>,
    pub portando : Option<isize>
}

#[derive(Clone, Copy, Debug)]
pub struct MPOp {
    pub note : Option<MPNote>,       // Hold, if None
    pub pticks : usize,
}

impl fmt::Display for MPOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	if let Some(MPNote { note, timbre, portando } ) = self.note {
	    let timbre = if let Some(MPTimbre { timbre : t, instrument  }) = timbre {
		if let Some(i) = instrument {
		    format!("_timb.{t}+ins.{i}")
		} else {
		    format!("_timb.{t}")
		}
	    } else {
		"".to_string()
	    };
	    let portando = if let Some(p) = portando {
		format!("~port~{p}")
	    } else {
		"".to_string()
	    };
            write!(f, "N{note}{timbre}{portando},{}t", self.pticks)
	} else {
	    write!(f, "Hold,{}t", self.pticks)
	}
    }
}

#[derive(Clone)]
pub struct Monopattern {
    pub ops : Vec<MPOp>,
}

impl fmt::Display for Monopattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f, "{}", fmt_slice(&self.ops[..]))
    }
}

// ================================================================================
// Divisions

#[derive(Clone, Copy)]
pub enum DivisionEffect {
    TimbreAdjust(usize),
    FullStop,
    ChannelSpeed(usize),
    ChannelVolume(usize), // from 1 to 64, inclusive
}

impl fmt::Display for DivisionEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	match self {
	    DivisionEffect::TimbreAdjust(t)  => write!(f, "tmb+{t:02x}"),
	    DivisionEffect::FullStop         => write!(f, "-STOP-"),
	    DivisionEffect::ChannelSpeed(s)  => write!(f, "spd={s:02x}"),
	    DivisionEffect::ChannelVolume(v) => write!(f, "vol:{v:02x}"),
	}
    }
}

#[derive(Clone, Copy)]
pub struct DivisionChannel {
    pub monopat   : usize,
    pub transpose : isize,
    pub effect    : DivisionEffect,
}

impl fmt::Display for DivisionChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let sign = if self.transpose < 0 { "-".to_string() } else { "+".to_string() };
	write!(f, "P#{:02x}{}{:02x}_{}",
	       self.monopat, sign, isize::abs(self.transpose), self.effect)
    }
}

impl DivisionChannel {
    pub fn empty() -> DivisionChannel {
	DivisionChannel {
	    monopat : 0, transpose : 0, effect : DivisionEffect::FullStop
	}
    }
}

#[derive(Clone, Copy)]
pub struct Division {
    pub channels : [DivisionChannel; 4],
}

impl fmt::Display for Division {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f,"{:>17} {:>17} {:>17} {:>17}",
	       format!("{}", self.channels[0]),
	       format!("{}", self.channels[1]),
	       format!("{}", self.channels[2]),
	       format!("{}", self.channels[3]))
    }
}

// ================================================================================
// Divisions

#[derive(Clone, Copy)]
pub struct SongInfo {
    pub first_division : usize,
    pub last_division : usize,
    pub speed : usize,
}

impl fmt::Display for SongInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f,"[Div #{:02x}--#{:02x}, speed={}]",
	       self.first_division,
	       self.last_division,
	       self.speed)
    }
}

// ================================================================================
// Song

pub struct Song {
    pub basic_samples : Vec<BasicSample>,
    //   pub slide_samples : Vec<Vec<SampleRange>>, // Samples used by Slide instrument effects
    pub instruments : Vec<Instrument>,
    pub timbres : Vec<Timbre>,
    pub monopatterns : Vec<Monopattern>,
    pub divisions : Vec<Division>,
    pub songinfo : SongInfo,
}

impl fmt::Display for Song {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	write!(f, "##[ Basic Samples ]\n")?;
	for (n, bs) in self.basic_samples.iter().enumerate() {
	    write!(f, "- S:{n:x}: {bs}\n")?;
	}
	write!(f, "##[ Instruments ]\n")?;
	for (n, instr) in self.instruments.iter().enumerate() {
	    write!(f, "- I:{n:x}: {instr}\n")?;
	}
	write!(f, "##[ Timbres ]\n")?;
	for (n, timbre) in self.timbres.iter().enumerate() {
	    write!(f, "- T:{n:x}: {timbre}\n")?;
	}
	write!(f, "##[ Monopatterns ]\n")?;
	for (n, monopat) in self.monopatterns.iter().enumerate() {
	    write!(f, "- P{n:02x}:\t{monopat}\n")?;
	}
	write!(f, "##[ Divisions ]\n")?;
	for (n, div) in self.divisions.iter().enumerate() {
	    write!(f, "- D{n:02x}:\t{div}\n")?;
	}
	return write!(f, "##[ {} ]\n", self.songinfo)
    }
}

struct TableIndexedData<'a> {
    data : &'a [u8],
    count : usize,
    start : usize,
    end : usize,
}

// ================================================================================
// Decoding

// --------------------------------------------------------------------------------
// Raw song access

#[derive(Clone, Copy)]
pub struct RawSection {
    pos : usize,
    num : usize,
    end : usize,
}

impl RawSection {
    fn new(pos : u32, num : u16, end : usize) -> RawSection {
	return RawSection { pos : pos as usize, num : num as usize, end };
    }
}

impl fmt::Display for RawSection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:x}..0x{:x}\t({}..{})\twith 0x{:x} ({}) entries]",
	       self.pos, self.end, self.pos, self.end, self.num, self.num)
    }
}

pub struct RawSong<'a> {
    data : &'a [u8],
    instruments  : RawSection,
    timbres      : RawSection,
    monopatterns : RawSection,
    divisions    : RawSection,
    subsongs     : RawSection,
    samples      : RawSection,
}

impl<'a> RawSong<'a> {
    fn new(data_pos : usize, data : &'a [u8]) -> RawSong<'a> {
	let pos_end = decode::u32(data, 28) as usize;
	let samples      = RawSection::new(decode::u32(data, 24), decode::u16(data, 50), pos_end);
	let subsongs     = RawSection::new(decode::u32(data, 20), decode::u16(data, 48), samples.pos);
	let divisions    = RawSection::new(decode::u32(data, 16), decode::u16(data, 42) + 1, subsongs.pos);
	let monopatterns = RawSection::new(decode::u32(data, 12), decode::u16(data, 40) + 1, divisions.pos);
	let timbres      = RawSection::new(decode::u32(data,  8), decode::u16(data, 38) + 1, monopatterns.pos);
	let instruments  = RawSection::new(decode::u32(data,  4), decode::u16(data, 36) + 1, timbres.pos);
	pinfo!("--  Song at {:x}:", data_pos);
	for (n, d) in [("instruments", instruments),
		       ("timbres", timbres),
		       ("monopatterns", monopatterns),
		       ("divisions", divisions),
		       ("subsongs", subsongs),
		       ("samples", samples)] {
	    pinfo!("  {n:12} {d}");
	}
	return RawSong {
	    data, samples, subsongs, divisions, monopatterns, timbres, instruments,
	}
    }

    fn subslice(&self, sec : RawSection, size : usize, i : usize) -> &'a [u8] {
	let p = sec.pos + size * i;
	return &self.data[p..p + size];
    }

    fn table_index(&self, sec : RawSection) -> TableIndexedData<'a> {
	return TableIndexedData::new(self.data, sec.pos, sec.end, sec.num);
    }

    fn basic_samples(&self) -> Vec<BasicSample> {
	let mut result = vec![];
	for i in 0..self.samples.num {
	    let d = self.subslice(self.samples, 10, i);
	    let pos = decode::u32(d, 0) as usize;
	    let length = (decode::u16(d, 4) as usize) << 1;
	    let loop_ptr = pos + decode::u16(d, 6) as usize;
	    let repeat = decode::u16(d, 8);
	    let repeat_end = (repeat as usize) << 1;

	    let looping = if repeat <= 1 {
		// no repeat
		None
	    } else {
		Some(SampleRange::new(loop_ptr, repeat_end))
	    };
	    let sample = BasicSample { attack  : SampleRange::new(pos, length),
				       looping, };
	    pinfo!("  Sample #{i}:\t{}", sample);
	    result.push(sample);
	}
	return result;
    }

    fn instruments(&self, basic_samples : &Vec<BasicSample>) -> Vec<Instrument> {
	let mut result : Vec<Instrument> = vec![];
	let instrument_table = self.table_index(self.instruments);
	for mut raw_ins in instrument_table {
	    let mut ops = vec![];
	    let mut pos_map = HashMap::new();
	    let mut goto_label = None;

	    if raw_ins.at_end() {
		pinfo!("Empty instrument definition after {} instruments, stopping",
		      result.len());
		break;
	    }

	    loop {
		pos_map.insert(raw_ins.relative_offset(), ops.len());

		if raw_ins.at_end() {
		    warn!("Prematurely reached end of block");
		    break;
		}

		const OP_LOOP : u8          = 0xe0;
		const OP_COMPLETED : u8     = 0xe1;
		const OP_SAMPLE : u8        = 0xe2;
		const OP_VIBRATO : u8       = 0xe3;
		const OP_SAMPLE_BRK : u8    = 0xe4;
		const OP_SLIDER : u8        = 0xe5;
		const OP_SLIDER_SUB : u8    = 0xe6;
		const OP_SAMPLE_VOL : u8    = 0xe7;
		const OP_WAIT : u8          = 0xe8;
		const OP_SAMPLE_CUSTOM : u8 = 0xe9;

		match raw_ins.u8() {
		    OP_LOOP => {
			let newpos = raw_ins.u8() as usize;
			goto_label = Some(newpos);
			break;     // done: loop
		    },
		    OP_COMPLETED => break, // done: no loop
		    OP_SAMPLE => {
			ops.push(InstrumentOp::StopSample);
			ops.push(InstrumentOp::Sample(basic_samples[raw_ins.u8() as usize]));
		    },
		    OP_SLIDER => {
			let sample_index = raw_ins.u8() as usize;
			let sample = basic_samples[sample_index].attack;
			let loop_pos_raw = raw_ins.u16();
			let len = (raw_ins.u16() as usize) << 1;
			let loop_start =
			    if loop_pos_raw == 0xffff {
				sample.start + sample.len - len
			    } else {
				(loop_pos_raw as usize) << 1
			    };
			let pos_delta = (raw_ins.u16() as i16 as isize) << 1;
			let ticks_delay = raw_ins.u8() as usize;
			ops.push(InstrumentOp::Slide(SlidingSample {
			    bounds : sample,
			    subsample_start : SampleRange::new(loop_start + sample.start, len),
			    delta: pos_delta,
			    delay_ticks : ticks_delay
			}));
			ops.push(InstrumentOp::ResetVolume);
		    }

		    OP_SAMPLE_VOL => {
			ops.push(InstrumentOp::Sample(basic_samples[raw_ins.u8() as usize]));
			ops.push(InstrumentOp::ResetVolume);
		    },

		    // --------------------
		    // Unsupported
		    OP_VIBRATO => {
			let vibspeed = raw_ins.u8();
			let vibdepth = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E3({vibspeed}, {vibdepth})")));
		    },

		    OP_SAMPLE_BRK => {
			let sample = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E4({sample})")));
		    },

		    OP_SLIDER_SUB => {
			let len = (raw_ins.u16()) << 1;
			let delta = (raw_ins.u16() as i16) << 1;
			let speed = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E6({len}, {delta}, {speed})")));
		    },

		    OP_WAIT => {
			let delay = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E8({delay})")));
		    },

		    OP_SAMPLE_CUSTOM => {
			let sample = raw_ins.u8();
			let index = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E9({sample}, {index})")));
		    },

		    // --------------------
		    // Default
		    transpose => {
			if transpose & 0x80 == 0x80 {
			    ops.push(InstrumentOp::FixedNote(transpose & !0x80));
			} else {
			    ops.push(InstrumentOp::Pitch(transpose as i8));
			}
			ops.push(InstrumentOp::WaitTicks(1));
		    },
		}
	    }
	    // Done decoding the instrument ops.  Now check if it ends in a loop:
	    match goto_label {
		None        => {},
		Some(label) => match pos_map.get(&label) {
		    None => {
			perror!("Instrument definition at 0x{:x} wants to go to bad offset 0x{:x} / {}!",
			       raw_ins.start, label, label)},
		    Some(ops_index) => {
			let lhs = &ops[..*ops_index];
			let rhs = &ops[*ops_index..];
			if rhs.len() > 0 {
			    let mut newops = lhs.to_vec();
			    newops.push(InstrumentOp::Loop(rhs.to_vec()));
			    ops = newops;
			}
		    }
		}
	    }
	    while let Some(InstrumentOp::WaitTicks(_)) = ops.last() {
		ops.pop();
	    }
	    let instrument = Instrument { ops };
	    pinfo!("Instrument #{} (0x{:x}) : {instrument}", result.len(), raw_ins.start);
	    result.push(instrument);
	} // looping over instrument table
	return result;
    }

    fn timbres(&self) -> Vec<Timbre> {
	let mut result : Vec<Timbre> = vec![];
	let timbre_table = self.table_index(self.timbres);
	for mut raw_tmb in timbre_table {

	    if raw_tmb.available(6) {
		pinfo!("Empty timbre definition after {} timbres, stopping",
		      result.len());
		break;
	    }

	    let vol_envelope_default_duration = raw_tmb.u8();
	    let instrument = {
		let i = raw_tmb.u8();
	        if i == 0x80 { None } else { Some(i) }
	    };
	    let vibrato = {
		let slope = (raw_tmb.u8() as i8) as isize;
		let depth = (raw_tmb.u8() as i8) as isize;
		Vibrato { slope, depth }
	    };
	    let vibrato_delay = raw_tmb.u8() as usize;

	    let mut ops = vec![];
	    let mut pos_map = HashMap::new();
	    let mut goto_label = None;
	    let mut duration = vol_envelope_default_duration as usize;

	    while !raw_tmb.at_end() {
		pos_map.insert(raw_tmb.relative_offset(), ops.len());
		let op = raw_tmb.u8();

		const OP_SUSTAIN : u8 = 0xe8;
		const OP_LOOP    : u8 = 0xe0;

		match op {
		    OP_SUSTAIN => {
			duration = raw_tmb.u8() as usize;
		    },
		    // End of envelope
		    0xe1 | 0xe2 | 0xe3 | 0xe4 | 0xe5 | 0xe6 | 0xe7 => {
			break;
		    },
		    OP_LOOP => {
			goto_label = Some((raw_tmb.u8() as isize) - 5);
			break;
		    },
		    volume  => ops.push(VolumeSpec{ volume, duration }),
		}
	    }

	    let mut loop_ops = vec![];

	    match goto_label {
		None        => {},
		Some(label) => match pos_map.get(&(label as usize)) {
		    None => {
			perror!("Timbre definition at 0x{:x} wants to go to bad offset 0x{:x} / {}!",
			       raw_tmb.start, label, label)},
		    Some(ops_index) => {
			let lhs = &ops[..*ops_index];
			let rhs = &ops[*ops_index..];
			if rhs.len() > 0 {
			    loop_ops = rhs.to_vec();
			    ops = lhs.to_vec();
			}
		    }
		}
	    }

	    result.push(Timbre {
		envelope_speed : vol_envelope_default_duration,
		instrument,
		vibrato,
		vibrato_delay,
		vol : VolumeEnvelope {
		    attack  : ops,
		    sustain : loop_ops,
		}
	    });
	    pinfo!("Timbre #{} (0x{:x}) : {}", result.len() - 1, raw_tmb.start, result.last().unwrap());

	}
	return result;
    }


    fn monopatterns(&self) -> Vec<Monopattern> {
	let mut result : Vec<Monopattern> = vec![];
	let monopattern_table = self.table_index(self.monopatterns);
	for mut raw_mp in monopattern_table {
	    if raw_mp.at_end() {
		pinfo!("Empty monopattern definition after {} monopatterns, stopping",
		      result.len());
		break;
	    }

	    let mut ops = vec![];
	    let mut duration : usize = 1; // Default # ticks

	    while !raw_mp.at_end() {
		let op = raw_mp.u8();

		const OP_END             : u8 = 0xff;
		const OP_SET_SPEED       : u8 = 0xfe;
		const OP_SET_SPEED_WAIT  : u8 = 0xfd;

		match op {
		    OP_END => {
			break;
		    },
		    OP_SET_SPEED => {
			duration = 1 + (raw_mp.u8() as usize);
		    },
		    OP_SET_SPEED_WAIT => {
			duration = 1 + (raw_mp.u8() as usize);
			ops.push(MPOp { note : None, pticks : duration });
		    },
		    note => {
			let note = note as i8;
			let raw_timbre = raw_mp.u8();

			let mut timbre = None;
			let mut portando = None;

			if note > 0 {
			    let timbre_index = raw_timbre & 0x1f;
			    timbre = Some (MPTimbre { timbre : timbre_index as usize,
						      instrument : None });

			    if raw_timbre & 0xe0 != 0 {
				let effect = raw_mp.u8() as i8;

				if raw_timbre & 0x40 == 0x40 {
				    timbre = Some (MPTimbre { timbre : timbre_index as usize,
							      instrument : Some(effect as usize) });
				}
				if raw_timbre & 0x20 == 0x20 {
				    portando = Some(-(effect as isize));
				}
			    }
			}
			ops.push(MPOp{ note   : Some(MPNote { note : note as isize, timbre, portando }),
				       pticks : duration });
		    }
		}
	    }

	    result.push(Monopattern {
		ops,
	    });
	    pinfo!("Monopattern #0x{:02x} (0x{:x}) : {}", result.len() - 1, raw_mp.start, result.last().unwrap());

	}
	return result;
    }

    fn divisions(&self) -> Vec<Division> {
	let mut result : Vec<Division> = vec![];
	for div_id in 0..self.divisions.num {
	    let ddata_pos = self.divisions.pos + 12 * div_id;
	    let ddata = &self.data[ddata_pos..ddata_pos + 12];
	    let mut chan_data : [DivisionChannel; 4] = [DivisionChannel::empty(); 4];

	    for chan_id in 0..4 {
		let cdata = &ddata[chan_id * 3..];
		let monopat = cdata[0] as usize;
		let transpose = (cdata[1] as i8) as isize;
		let raw_effect = cdata[2];
		let effect;

		if raw_effect & 0x80 == 0x80 {
		    let effect_type = (raw_effect & 0x70) >> 4;
		    let effect_value = raw_effect & 0xf;

		    const OP_FULLSTOP : u8 = 0x0;
		    const OP_SPEED    : u8 = 0x6;
		    const OP_VOLUME   : u8 = 0x7;

		    match effect_type {
			OP_FULLSTOP => effect = DivisionEffect::FullStop,
			OP_SPEED    => effect = DivisionEffect::ChannelSpeed((effect_value as usize) + 1),
			OP_VOLUME   => effect = DivisionEffect::ChannelVolume(64 - (effect_value as usize)),
			_           => {
			    effect = DivisionEffect::TimbreAdjust(0);
			    perror!("Unknown division effect type {effect_type}");
			}
		    }
		} else {
		    effect = DivisionEffect::TimbreAdjust(raw_effect as usize);
		}

		chan_data[chan_id] = DivisionChannel{
		    monopat, transpose, effect,
		};
	    }

	    result.push(Division { channels : chan_data });
	    pinfo!("Division #0x{:02x} (0x{:x}) : {}", result.len() - 1, ddata_pos, result.last().unwrap());

	}
	return result;
    }

    fn songs(&self) -> Vec<SongInfo> {
	let mut result : Vec<SongInfo> = vec![];
	for song_id in 0..self.subsongs.num {
	    let sdata_pos = self.subsongs.pos + 6 * song_id;
	    let sdata = &self.data[sdata_pos..sdata_pos + 12];
	    let first_division = decode::u16(sdata, 0) as usize;
	    let last_division = decode::u16(sdata, 2) as usize;
	    let speed = decode::u16(sdata, 4) as usize;

	    result.push(SongInfo { first_division, last_division, speed });
	    pinfo!("SongInfo #0x{:02x} (0x{:x}) : {}", result.len() - 1, sdata_pos, result.last().unwrap());

	}
	return result;
    }
}

// --------------------------------------------------------------------------------
// Table-based indexing

/// A chunk of data prefixed by a 16 bit index table
impl<'a> TableIndexedData<'a> {
    fn new(data : &'a [u8], start : usize, end : usize, count : usize) -> TableIndexedData<'a> {
	let result = TableIndexedData {
	    data,
	    count,
	    start,
	    end,
	};
	return result;
    }

    fn offset_of(&self, index : usize) -> usize {
	return decode::u16(self.data, self.start + 2 * index) as usize;
    }
}

impl<'a> std::iter::IntoIterator for TableIndexedData<'a> {
    type Item = TableIndexedElement<'a>;

    type IntoIter = TableIndexedIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
	let ret = TableIndexedIterator {
	    tdata : self,
	    index : 0,
	};
	return ret;
    }
}

struct TableIndexedIterator<'a> {
    tdata : TableIndexedData<'a>,
    index : usize,
}

struct TableIndexedElement<'a> {
    data : &'a [u8],
    current_pos : usize,
    start : usize,
    end_offset : usize, // first illegal positive offset delta on top of
}

impl<'a> std::iter::Iterator for TableIndexedIterator<'a> {
    type Item = TableIndexedElement<'a>;

    fn next(&mut self) -> Option<Self::Item> {
	if self.index >= self.tdata.count {
	    return Option::None;
	}
	let pos = self.tdata.offset_of(self.index);
	let end_pos = if self.index + 1 >= self.tdata.count { self.tdata.end } else { self.tdata.offset_of(self.index + 1) };
	let result = TableIndexedElement {
	    data : self.tdata.data,
	    start : pos,
	    current_pos : pos,
	    end_offset : end_pos - pos,
	};
	self.index += 1;
	return Some(result);
    }
}

impl<'a> TableIndexedElement<'a> {
    fn at_end(&self) -> bool {
	return self.available(0);
    }

    fn available(&self, d : usize) -> bool {
	return self.current_pos + d >= self.start + self.end_offset;
    }

    fn end(&self) -> usize {
	return self.start + self.end_offset;
    }

    fn step(&mut self, size : usize) {
	self.current_pos += size;
	if self.current_pos > self.end() {
	    perror!("Stepped outside of TableIndexedElement: {:x} / {:x} ", self.current_pos, self.end());
	}
    }

    fn relative_offset(&self) -> usize {
	return self.current_pos - self.start;
    }

    fn u8(&mut self) -> u8 {
	let v = self.data[self.current_pos];
	self.step(1);
	return v;
    }

    fn u16(&mut self) -> u16 {
	let v = decode::u16(self.data, self.current_pos);
	self.step(2);
	return v;
    }
}



// --------------------------------------------------------------------------------
// Finding song data

pub struct SongSeeker<'a> {
    data : &'a [u8],
    pos : usize,
    count : usize,
}

pub fn seeker<'a>(data : &'a [u8], start : usize) -> SongSeeker<'a> {
    let seeker = SongSeeker {
	data,
	pos : start,
	count : 0,
    };
    return seeker;
}

impl<'a> SongSeeker<'a> {
    pub fn next(&mut self) -> Option<Song> {
	// Based on code from Christian Corti
	let max = self.data.len();
	let mut npos = self.pos;

	while npos + 4 < max
	    && &self.data[npos..npos+4] != [b'C', b'O', b'S', b'O'] {
		npos += 1;
	    }
	self.pos = npos + 4;

	if npos + 4 >= max {
	    return None;
	}

	pinfo!("-------------------- Found song #{} at {:x}", self.count, npos);
	self.count += 1;

	let data = &self.data[npos..];

	let rawsong = RawSong::new(npos, data);

	let basic_samples = rawsong.basic_samples();
	let instruments = rawsong.instruments(&basic_samples);
	let timbres = rawsong.timbres();
	let monopatterns = rawsong.monopatterns();
	let divisions = rawsong.divisions();
	let songs = rawsong.songs();
	if songs.len() != 1 {
	    perror!("Unexpected number of songs: {}", songs.len());
	    if songs.len() == 0 {
		return None;
	    }
	}

	// Found a song header!
	return Some(Song{
	    basic_samples,
	    instruments,
	    timbres,
	    monopatterns,
	    divisions,
	    songinfo : songs[0],
	});
    }
}
