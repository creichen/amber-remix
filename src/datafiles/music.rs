#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use core::fmt;
use std::collections::HashMap;
use crate::{datafiles::decode, audio::{SampleRange, Freq}};

// ================================================================================
// Frequencies

// CoSo period values
pub const PERIODS : [u16; 7 * 12] = [
    1712 , 1616 , 1524 , 1440 , 1356 , 1280 , 1208 , 1140 , 1076 , 1016 ,   960 ,   906,
    856  ,  808 ,  762 ,  720 ,  678 ,  640 ,  604 ,  570 ,  538 ,  508 ,   480 ,   453,
    428  ,  404 ,  381 ,  360 ,  339 ,  320 ,  302 ,  285 ,  269 ,  254 ,   240 ,   226,
    214  ,  202 ,  190 ,  180 ,  170 ,  160 ,  151 ,  143 ,  135 ,  127 ,   120 ,   113,
    113  ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,  113 ,   113 ,   113,
    3424 , 3232 , 3048 , 2880 , 2712 , 2560 , 2416 , 2280 , 2152 , 2032 ,  1920 ,  1812,
    6848 , 6464 , 6096 , 5760 , 5424 , 5120 , 4832 , 4560 , 4304 , 4064 ,  3840 ,  3624];

pub fn period_to_freq(period : u16) -> Freq {
    return (3546894.6 / period as f32) as Freq;
}

// ================================================================================
// Samples

#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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

#[derive(Clone)]
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
	    InstrumentOp::Loop(vec)        => write!(f, "loop[{}]", InstrumentOp::fmt_slice(&vec)),
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

pub struct Instrument {
    ops : Vec<InstrumentOp>,
}

impl fmt::Display for Instrument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
	let mut s = "".to_string();
	write!(f, "{}", InstrumentOp::fmt_slice(&self.ops[..]))
    }
}

impl InstrumentOp {
    fn fmt_slice(v : &[InstrumentOp]) -> String {
	let mut s = "".to_string();
	for o in v {
	    if s.len() > 0 {
		s.push_str("   ");
	    }
	    let str = format!("{}", o);
	    s.push_str(&str);
	}
	return s;
    }
}

// ================================================================================
// Song


pub struct Song {
    pub basic_samples : Vec<BasicSample>,
    //   pub slide_samples : Vec<Vec<SampleRange>>, // Samples used by Slide instrument effects
    pub instruments : Vec<Instrument>,
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
    data_pos : usize,
    instruments  : RawSection,
    timbres      : RawSection,
    monopatterns : RawSection,
    divisions    : RawSection,
    songs        : RawSection,
    samples      : RawSection,
}

impl<'a> RawSong<'a> {
    fn new(data_pos : usize, data : &'a [u8]) -> RawSong<'a> {
	let pos_end = decode::u32(data, 28) as usize;
	let samples      = RawSection::new(decode::u32(data, 24), decode::u16(data, 50), pos_end);
	let songs        = RawSection::new(decode::u32(data, 20), decode::u16(data, 48), samples.pos);
	let divisions    = RawSection::new(decode::u32(data, 16), decode::u16(data, 42) + 1, songs.pos);
	let monopatterns = RawSection::new(decode::u32(data, 12), decode::u16(data, 40) + 1, divisions.pos);
	let timbres      = RawSection::new(decode::u32(data,  8), decode::u16(data, 38) + 1, monopatterns.pos);
	let instruments  = RawSection::new(decode::u32(data,  4), decode::u16(data, 36) + 1, timbres.pos);
	info!("--  Song at {:x}:", data_pos);
	for (n, d) in [("instruments", instruments),
		       ("timbres", timbres),
		       ("monopatterns", monopatterns),
		       ("divisions", divisions),
		       ("songs", songs),
		       ("samples", samples)] {
	    info!("  {n:12} {d}");
	}
	return RawSong {
	    data, data_pos, samples, songs, divisions, monopatterns, timbres, instruments,
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
	    info!("  Sample #{i}:\t{}", sample);
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
		info!("Empty instrument definition after {} instruments, stopping",
		      result.len());
		break;
	    }

	    loop {
		pos_map.insert(raw_ins.relative_offset(), ops.len());

		if raw_ins.at_end() {
		    warn!("Prematurely reached end of block");
		    break;
		}

		match raw_ins.u8() {
		    0xe0 => {
			let newpos = raw_ins.u8() as usize;
			goto_label = Some(newpos);
			break;     // done: loop
		    },
		    0xe1 => break, // done: no loop
		    0xe2 => {
			ops.push(InstrumentOp::StopSample);
			ops.push(InstrumentOp::Sample(basic_samples[raw_ins.u8() as usize]));
		    },
		    0xe5 => {
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
			    subsample_start : SampleRange::new(loop_start, len),
			    delta: pos_delta,
			    delay_ticks : ticks_delay
			}));
			ops.push(InstrumentOp::ResetVolume);
		    }

		    0xe7 => {
			ops.push(InstrumentOp::Sample(basic_samples[raw_ins.u8() as usize]));
			ops.push(InstrumentOp::ResetVolume);
		    },

		    // --------------------
		    // Unsupported
		    0xe3 => {
			let vibspeed = raw_ins.u8();
			let vibdepth = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E3({vibspeed}, {vibdepth})")));
		    },

		    0xe4 => {
			let sample = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E4({sample})")));
		    },

		    0xe6 => {
			let len = (raw_ins.u16()) << 1;
			let delta = (raw_ins.u16() as i16) << 1;
			let speed = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E6({len}, {delta}, {speed})")));
		    },

		    0xe8 => {
			let delay = raw_ins.u8();
			ops.push(InstrumentOp::Unsupported(format!("E8({delay})")));
		    },

		    0xe9 => {
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
			error!("Instrument definition at 0x{:x} wants to go to bad offset 0x{:x} / {}!",
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
	    info!("Instrument #{} (0x{:x}) : {instrument}", result.len(), raw_ins.start);
	    result.push(instrument);
	} // looping over instrument table
	return result;
    }

	// // // -- ----------------------------------------
	// // // frqseqs -> Instruments
	// let frqseqs = TableIndexedData::new(data, pos_frqseqs, pos_volumes, num_freqs);
	// for f in frqseqs {
	//     print!("frqseq[{:x}] @ {:03x}:{:x} =  ",
	// 	   f.index, f.offset, npos + f.offset);
	//     let mut pos = 0;
	//     while pos < f.end_offset {
	// 	let insn = f.u8(pos);
	// 	pos += 1;
	// 	match insn {
	// 	    0xe0 => {
	// 		let newpos = f.u8(pos);
	// 		pos += 1;
	// 		print!("  GOTO({newpos}) ")
	// 	    },
	// 	    0xe1 => print!(" -STOP- "),
	// 	    0xe2 | 0xe4 | 0xe7 => {
	// 		let sample = f.u8(pos);
	// 		pos += 1;
	// 		print!("  {insn:x}_SET-SAMPLE({sample:x})");
	// 	    }
	// 	    0xe5 => {
	// 		let sample = f.u8(pos);
	// 		let sample_slide_loop = f.u16(pos + 1);
	// 		let sample_slide_len = f.u16(pos + 3);
	// 		let sample_slide_delta = f.u16(pos + 5) as i16;
	// 		let sample_slide_speed = f.u8(pos + 6);
	// 		pos += 1+7;
	// 		print!("  {insn:x}_SET-SAMPLE-SLIDE({sample:x}, speed={sample_slide_speed:x}, loop_pos/2={sample_slide_loop:x}, len/2={sample_slide_len:x}, delta={sample_slide_delta:x})");
	// 	    }
	// 	    // 0xe5 => {
	// 	    // 	let sample = f.u8(pos);
	// 	    // 	let sample_subindex = f.u8(pos + 1);
	// 	    // 	pos += 2;
	// 	    // 	print!("  {insn:x}_SET-SAMPLE-SUB({sample:x}, {sample_subindex:x})");
	// 	    // }
	// 	    0xe3 => {
	// 		let vibspeed = f.u8(pos);
	// 		let vibdepth = f.u8(pos + 1);
	// 		pos += 2;
	// 		print!("  VIBRATO({vibspeed} at {vibdepth}) ")
	// 	    },
	// 	    // 0xe6 also?
	// 	    //0xe5 => print!("  {insn:x}_UNSUPPORTED"),
	// 	    0xe6 => print!("  {insn:x}_UNSUPPORTED"),
	// 	    0xe8 => {
	// 		let delay = f.u8(pos);
	// 		pos += 1;
	// 		print!("  DELAY({delay}) ")
	// 	    },
	// 	    0xe9 => print!("  {insn:x}_UNSUPPORTED"),
	// 	    transpose => print!("  NOTE+({transpose}) "),
	// 	}
	//     }
	//     println!("");
	// }


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
    index : usize,
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
	    index : self.index,
	    start : pos,
	    current_pos : pos,
	    end_offset : end_pos - pos,
	};
	self.index += 1;
	return Some(result);
    }
}

impl<'a> TableIndexedElement<'a> {
    fn abs_u8(&self, pos : usize) -> u8 {
	return self.data[self.start + pos];
    }

    fn abs_u16(&self, pos : usize) -> u16 {
	return decode::u16(self.data, self.start + pos);
    }

    fn at_end(&self) -> bool {
	return self.current_pos >= self.start + self.end_offset;
    }

    fn end(&self) -> usize {
	return self.start + self.end_offset;
    }

    fn step(&mut self, size : usize) {
	self.current_pos += size;
	if self.current_pos > self.end() {
	    error!("Stepped outside of TableIndexedElement: {:} / {:} ", self.current_pos, self.end());
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
}

pub fn seeker<'a>(data : &'a [u8], start : usize) -> SongSeeker<'a> {
    let seeker = SongSeeker {
	data,
	pos : start,
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

	info!("-------------------- Found song at {:x}", npos);

	let data = &self.data[npos..];

	let rawsong = RawSong::new(npos, data);

	let basic_samples = rawsong.basic_samples();

	let instruments = rawsong.instruments(&basic_samples);

	// // -- ----------------------------------------
	// // Songs

	// let songs_data = &data[pos_song_data..pos_sample_data];
	// for i in 0..num_songs {
	//     let i6 = i * 6 as usize;
	//     let song_spec = &songs_data[i6..i6 + 6];
	//     let pos_song_start = decode::u16(song_spec, 0) as usize + pos_tracks;
	//     let pos_song_end = decode::u16(song_spec, 2) as usize + pos_tracks;
	//     let speed = decode::u16(song_spec, 4);
	//     println!("Song #{i} = {{ start {pos_song_start:x}, end = {pos_song_end:x}, speed {speed} }}");

	//     if speed > 0 {
	// 	let mut voices : Vec<Voice> = vec![];

	// 	for i in 0..4 {
	// 	    let voice = Voice {
	// 		track_ptr : (pos_song_start + i * 3) as usize,
	// 		track_pos : 0,
	// 		pattern_pos : 0,
	// 		transpose : 0,
	// 		coso_speed_factor : 1,
	// 	    };
	// 	    voices.push(voice);
	// 	}
	//     }
	// }

	// // -- ----------------------------------------
	// // tracks -> Divisions

	// let tracks = &data[pos_tracks..pos_song_data];
	// print!("      ");
	// for v in 0..4 {
	//     print!(" {v} PP  TRNS  EFFECT   ");
	// }
	// println!("");
	// for t in 0..num_tracks {
	//     print!("    ");
	//     for v in 0..4 {
	// 	let voffs = (v * 3) + (t * 12);
	// 	let vpat = &tracks[voffs..voffs+3];
	// 	//println!("{:x}: {:x} {:x} {:x}", voffs + npos, vpat[0], vpat[1], vpat[2]);
	// 	let new_pattern_pos = vpat[0];
	// 	let note_transpose = vpat[1] as i8;
	// 	let effect = vpat[2];
	// 	let effect_str : String;

	// 	/* magic-1: */
	// 	if effect & 0x80 == 0x80 {
	// 	    let effect_type = (effect >> 4) & 0x7;
	// 	    let effect_val = effect & 0xf;
	// 	    match effect_type {
	// 		0 => effect_str = " -STOP-".to_string(),
	// 		6 => effect_str = format!("SPEED={:01x}", effect_val),
	// 		7 => effect_str = {
	// 		    let fade_speed = if effect_val == 0 { 100 }  else { (16 - effect_val) * 6 };
	// 		    format!("FADE:{:2x}", fade_speed)
	// 		},
	// 		_ => effect_str = format!("???[{:02x}]", effect),
	// 	    }
	// 	} else {
	// 	    effect_str = format!("VOL+={:02x}", effect);
	// 	}
	// 	print!("     {new_pattern_pos:02x}  {note_transpose:4}  {effect_str}");
	//     }
	//     println!("");
	// }

	// // -- ----------------------------------------
	// // patterns -> Monopatterns
	// let patterns = TableIndexedData::new(data, pos_patterns, pos_tracks, num_patterns);
	// for p in patterns {
	//     let pattern_offset = p.offset;
	//     println!("--- pattern {:x} at offset {pattern_offset:x}={:x}", p.index, npos as u32 + pattern_offset as u32);
	//     let mut pos = 0;
	//     let mut insn = 0;
	//     while insn != 0xff {
	// 	insn = p.u8(pos);
	// 	pos += 1;
	// 	print!("\t");
	// 	match insn {
	// 	    0xff => println!("--end--"),
	// 	    0xfe | 0xfd => {
	// 		let channel_speed_factor = 1 + p.u8(pos) as u16;
	// 		pos += 1;
	// 		println!("c-speed = {channel_speed_factor}{}",
	// 			 if insn == 0xfe { " ...(cont)..." } else { "" });
	// 	    }
	// 	    _    => {
	// 		let note_info = insn as i8;
	// 		let note = note_info & 0x7f;
	// 		let basevolume_info = p.u8(pos);
	// 		let basevolume = basevolume_info & 0x1f;
	// 		pos += 1;
	// 		if note_info < 0 {
	// 		    print!("defer-");
	// 		}
	// 		print!("play {note:3} @ {basevolume}");
	// 		if basevolume_info & !0x1f != 0 {
	// 		    let effect_val = p.u8(pos);
	// 		    let mut extra = "".to_string();
	// 		    if basevolume_info & 0x40 != 0 {
	// 			extra = format!(" override-freq={effect_val:x}");
	// 		    }
	// 		    if basevolume_info & 0x20 != 0 {
	// 			extra = format!("{} portando~{effect_val}", extra);
	// 		    }
	// 		    pos += 1;
	// 		    print!("{extra}");
	// 		}
	// 		println!("");
	// 	    },
	// 	}
	//     }
	// }

	// // -- ----------------------------------------
	// // volumes -> Timbres and Volume Envelopes
	// let volumes = TableIndexedData::new(data, pos_volumes, pos_patterns, num_volumes);
	// for v in volumes {
	//     let vol_speed = v.u8(0);
	//     let frq_index = v.u8(1) as i8;
	//     let vibrato_speed = v.u8(2) as i8;
	//     let vibrato_depth = v.u8(3) as i8;
	//     let vibrato_delay = v.u8(4);

	//     println!("vol[{:03x}] @ {:03x}:{:x}  = {vol_speed:5o}  frq:{}  vibrato=[{vibrato_speed} at {vibrato_depth} after {vibrato_delay}]",
	// 	     v.index, v.offset,
	// 	     npos + v.offset, if frq_index == -128 { "#".to_string() } else { format!("{frq_index}") });

	//     let mut vol_pos = 5;
	//     let vol_end = v.end_offset;
	//     while vol_pos < vol_end {
	// 	let vol_insn = v.u8(vol_pos);
	// 	vol_pos += 1;
	// 	print!("\t{:02x}: ", vol_pos);
	// 	match vol_insn {
	// 	    0xe8 => {
	// 		let sustain = v.u8(vol_pos);
	// 		vol_pos += 1;
	// 		println!("\tsustain {sustain}")
	// 	    },
	// 	    0xe1 | 0xe2 | 0xe3 | 0xe4 | 0xe5 | 0xe6 | 0xe7 => {
	// 		println!("\t(maintain indefinitely)");
	// 	    }
	// 	    0xe0 => {
	// 		let pos = v.u8(vol_pos) as i8 - 5;
	// 		vol_pos += 1;
	// 		println!("\tgoto {pos:x}")
	// 	    },
	// 	    _ => println!("\tvol = {vol_insn}"),
	// 	}
	//     }
	// }

	// Found a song header!
	return Some(Song{
	    basic_samples,
	    instruments,
	});
    }
}
