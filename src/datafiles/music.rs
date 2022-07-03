use crate::datafiles::decode;

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


pub struct Song {
    
}

pub struct Voice {
    track_ptr : usize,
    track_pos : usize,
    pattern_pos : usize,
    transpose : i32,
    coso_speed_factor : u32,
}

struct TableIndexedData<'a> {
    data : &'a [u8],
    count : usize,
    start : usize,
    end : usize,
}

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
    index : usize,
    offset : usize,
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
	    offset : pos,
	    end_offset : end_pos - pos,
	};
	self.index += 1;
	return Some(result);
    }
}

impl<'a> TableIndexedElement<'a> {
    fn u8(&self, pos : usize) -> u8 {
	return self.data[self.offset + pos];
    }

    fn u16(&self, pos : usize) -> u16 {
	return decode::u16(self.data, self.offset + pos);
    }
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

	println!("-------------------- Found song at {:x}", npos);

	let data = &self.data[npos..];
	let pos_frqseqs = decode::u32(data, 4) as usize;
	let pos_volumes = decode::u32(data, 8) as usize;
	let pos_patterns = decode::u32(data, 12) as usize;
	let pos_tracks = decode::u32(data, 16) as usize;
	let pos_song_data = decode::u32(data, 20) as usize;
	let pos_sample_data = decode::u32(data, 24) as usize;
	let pos_end = decode::u32(data, 28) as usize;

	let num_freqs = decode::u16(data, 36) as usize;
	let num_volumes = (decode::u16(data, 38) + 1) as usize;
	let num_patterns = (decode::u16(data, 40) + 1) as usize;
	let num_tracks = (decode::u16(data, 42) + 1) as usize;
	let num_songs = decode::u16(data, 48) as usize;
	let num_samples = decode::u16(data, 50);

	println!("frqseqs at {:x}       (count={})", npos + pos_frqseqs, num_freqs);
	println!("volseqs at {:x}       (count={})", npos + pos_volumes, num_volumes);
	println!("patterns at {:x}      (count={})", npos + pos_patterns, num_patterns);
	println!("tracks at {:x}        (count={})", npos + pos_tracks, num_tracks);
	println!("song-data at {:x}     (count={})", npos + pos_song_data, num_songs);
	println!("sample-data at {:x}   (count={})", npos + pos_sample_data, num_samples);
	println!("end at {:x}", npos + pos_end);

	// -- ----------------------------------------
	// Samples

	let samples_header = &data[pos_sample_data..pos_end];
	for i in 0..num_samples {
	    // References into SAMPLEDA.IMG
	    let i10 = (i * 10) as usize;
	    let sample_spec = &samples_header[i10..i10+10];
	    let pointer = decode::u32(sample_spec, 0);
	    let length = (decode::u16(sample_spec, 4) as u32) << 1;
	    let loop_ptr = pointer + decode::u16(sample_spec, 6) as u32;
	    let repeat = (decode::u16(sample_spec, 8) as u32) << 1;

	    println!("sample #{i}:");
	    println!("  ptr = {pointer:x}");
	    println!("  len = {length:x} => endpos = {:x}", pointer + length);
	    println!("  loop_ptr = {loop_ptr:x}");
	    println!("  repeat   = {repeat} => loopened {:x}", loop_ptr + repeat);
	}

	// -- ----------------------------------------
	// Songs

	let songs_data = &data[pos_song_data..pos_sample_data];
	for i in 0..num_songs {
	    let i6 = i * 6 as usize;
	    let song_spec = &songs_data[i6..i6 + 6];
	    let pos_song_start = decode::u16(song_spec, 0) as usize + pos_tracks;
	    let pos_song_end = decode::u16(song_spec, 2) as usize + pos_tracks;
	    let speed = decode::u16(song_spec, 4);
	    println!("Song #{i} = {{ start {pos_song_start:x}, end = {pos_song_end:x}, speed {speed} }}");

	    if speed > 0 {
		let mut voices : Vec<Voice> = vec![];

		for i in 0..4 {
		    let voice = Voice {
			track_ptr : (pos_song_start + i * 3) as usize,
			track_pos : 0,
			pattern_pos : 0,
			transpose : 0,
			coso_speed_factor : 1,
		    };
		    voices.push(voice);
		}
	    }
	}

	// -- ----------------------------------------
	// tracks -> Divisions

	let tracks = &data[pos_tracks..pos_song_data];
	print!("      ");
	for v in 0..4 {
	    print!(" {v} PP  TRNS  EFFECT   ");
	}
	println!("");
	for t in 0..num_tracks {
	    print!("    ");
	    for v in 0..4 {
		let voffs = (v * 3) + (t * 12);
		let vpat = &tracks[voffs..voffs+3];
		//println!("{:x}: {:x} {:x} {:x}", voffs + npos, vpat[0], vpat[1], vpat[2]);
		let new_pattern_pos = vpat[0];
		let note_transpose = vpat[1] as i8;
		let effect = vpat[2];
		let effect_str : String;

		/* magic-1: */
		if effect & 0x80 == 0x80 {
		    let effect_type = (effect >> 4) & 0x7;
		    let effect_val = effect & 0xf;
		    match effect_type {
			0 => effect_str = " -STOP-".to_string(),
			6 => effect_str = format!("SPEED={:01x}", effect_val),
			7 => effect_str = {
			    let fade_speed = if effect_val == 0 { 100 }  else { (16 - effect_val) * 6 };
			    format!("FADE:{:2x}", fade_speed)
			},
			_ => effect_str = format!("???[{:02x}]", effect),
		    }
		} else {
		    effect_str = format!("VOL+={:02x}", effect);
		}
		print!("     {new_pattern_pos:02x}  {note_transpose:4}  {effect_str}");
	    }
	    println!("");
	}

	// -- ----------------------------------------
	// patterns -> Monopatterns
	let patterns = TableIndexedData::new(data, pos_patterns, pos_tracks, num_patterns);
	for p in patterns {
	    let pattern_offset = p.offset;
	    println!("--- pattern {:x} at offset {pattern_offset:x}={:x}", p.index, npos as u32 + pattern_offset as u32);
	    let mut pos = 0;
	    let mut insn = 0;
	    while insn != 0xff {
		insn = p.u8(pos);
		pos += 1;
		print!("\t");
		match insn {
		    0xff => println!("--end--"),
		    0xfe | 0xfd => {
			let channel_speed_factor = 1 + p.u8(pos) as u16;
			pos += 1;
			println!("c-speed = {channel_speed_factor}{}",
				 if insn == 0xfe { " ...(cont)..." } else { "" });
		    }
		    _    => {
			let note_info = insn as i8;
			let note = note_info & 0x7f;
			let basevolume_info = p.u8(pos);
			let basevolume = basevolume_info & 0x1f;
			pos += 1;
			if note_info < 0 {
			    print!("defer-");
			}
			print!("play {note:3} @ {basevolume}");
			if basevolume_info & !0x1f != 0 {
			    let effect_val = p.u8(pos);
			    let mut extra = "".to_string();
			    if basevolume_info & 0x40 != 0 {
				extra = format!(" override-freq={effect_val:x}");
			    }
			    if basevolume_info & 0x20 != 0 {
				extra = format!("{} portando~{effect_val}", extra);
			    }
			    pos += 1;
			    print!("{extra}");
			}
			println!("");
		    },
		}
	    }
	}

	// -- ----------------------------------------
	// volumes -> Timbres and Volume Envelopes
	let volumes = TableIndexedData::new(data, pos_volumes, pos_patterns, num_volumes);
	for v in volumes {
	    let vol_speed = v.u8(0);
	    let frq_index = v.u8(1) as i8;
	    let vibrato_speed = v.u8(2) as i8;
	    let vibrato_depth = v.u8(3) as i8;
	    let vibrato_delay = v.u8(4);

	    println!("vol[{:03x}] @ {:03x}:{:x}  = {vol_speed:5o}  frq:{}  vibrato=[{vibrato_speed} at {vibrato_depth} after {vibrato_delay}]",
		     v.index, v.offset,
		     npos + v.offset, if frq_index == -128 { "#".to_string() } else { format!("{frq_index}") });

	    let mut vol_pos = 5;
	    let vol_end = v.end_offset;
	    while vol_pos < vol_end {
		let vol_insn = v.u8(vol_pos);
		vol_pos += 1;
		print!("\t{:02x}: ", vol_pos);
		match vol_insn {
		    0xe8 => {
			let sustain = v.u8(vol_pos);
			vol_pos += 1;
			println!("\tsustain {sustain}")
		    },
		    0xe1 | 0xe2 | 0xe3 | 0xe4 | 0xe5 | 0xe6 | 0xe7 => {
			println!("\t(maintain indefinitely)");
		    }
		    0xe0 => {
			let pos = v.u8(vol_pos) as i8 - 5;
			vol_pos += 1;
			println!("\tgoto {pos:x}")
		    },
		    _ => println!("\tvol = {vol_insn}"),
		}
	    }
	}

	// // -- ----------------------------------------
	// // frqseqs -> Instruments
	let frqseqs = TableIndexedData::new(data, pos_frqseqs, pos_volumes, num_freqs);
	for f in frqseqs {
	    print!("frqseq[{:x}] @ {:03x}:{:x} =  ",
		   f.index, f.offset, npos + f.offset);
	    let mut pos = 0;
	    while pos < f.end_offset {
		let insn = f.u8(pos);
		pos += 1;
		match insn {
		    0xe0 => {
			let newpos = f.u8(pos);
			pos += 1;
			print!("  GOTO({newpos}) ")
		    },
		    0xe1 => print!(" -STOP- "),
		    0xe2 | 0xe4 | 0xe7 => {
			let sample = f.u8(pos);
			pos += 1;
			print!("  {insn:x}_SET-SAMPLE({sample:x})");
		    }
		    0xe5 => {
			let sample = f.u8(pos);
			let sample_slide_loop = f.u16(pos + 1);
			let sample_slide_len = f.u16(pos + 3);
			let sample_slide_delta = f.u16(pos + 5) as i16;
			let sample_slide_speed = f.u8(pos + 6);
			pos += 1+7;
			print!("  {insn:x}_SET-SAMPLE-SLIDE({sample:x}, speed={sample_slide_speed:x}, loop_pos/2={sample_slide_loop:x}, len/2={sample_slide_len:x}, delta={sample_slide_delta:x})");
		    }
		    // 0xe5 => {
		    // 	let sample = f.u8(pos);
		    // 	let sample_subindex = f.u8(pos + 1);
		    // 	pos += 2;
		    // 	print!("  {insn:x}_SET-SAMPLE-SUB({sample:x}, {sample_subindex:x})");
		    // }
		    0xe3 => {
			let vibspeed = f.u8(pos);
			let vibdepth = f.u8(pos + 1);
			pos += 2;
			print!("  VIBRATO({vibspeed} at {vibdepth}) ")
		    },
		    // 0xe6 also?
		    //0xe5 => print!("  {insn:x}_UNSUPPORTED"),
		    0xe6 => print!("  {insn:x}_UNSUPPORTED"),
		    0xe8 => {
			let delay = f.u8(pos);
			pos += 1;
			print!("  DELAY({delay}) ")
		    },
		    0xe9 => print!("  {insn:x}_UNSUPPORTED"),
		    transpose => print!("  NOTE+({transpose}) "),
		}
	    }
	    println!("");
	}

	// Found a song header!
	return Some(Song{ } );
    }
}
