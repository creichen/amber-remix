// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use core::fmt;
use std::assert;
use std::fs::File;
use std::cmp::min;
use std::io::Read;
use std::path::Path;

use crate::datafiles::pixmap::Pixmap;
use crate::datafiles::palette::Palette;

use self::music::Song;
use self::pixmap::IndexedPixmap;
use self::tile::Tileset;
use self::map::Map;

const DEBUG : bool = true;

mod string_fragment_table;
mod map_string_table;
mod decode;
mod bytepattern;
mod pictures;
pub mod amber_string;
pub mod palette;
pub mod pixmap;
pub mod music;
pub mod sampledata;
pub mod tile;
pub mod map;
pub mod labgfx;

#[derive(Debug)]
pub enum FileHeaderType {
    LOB, // = VOL1
    // As file header:
    //     [hdr:32]
    //     [decompsize:32]  // only lower 24 bit
    //     ... (lob-compressed data)
    // (also in AMNC, AMNP, AMPC
    JH(u16),
    // As file header:
    //     [hdr=JH(x):32]
    //     ... (JH(x)-compressed data, may be any other container below)
    // (implicitly AMNC, AMNP)

    AMNC, // seems to be AmberMoon-only
    // ONLY as file header:
    //     [hdr:32]
    //     [num_files:16]
    //     [compressedsize_1:32]
    //        ...
    //     [compressedsize_num_files:32]
    //     ... (filedata_1, implicitly JH(1)-encoded)  (after decoding: LOB/VOL1, or RAW)
    //        ...
    //     ... (filedata_num_files, implicitly JH(num_files)-encoded)  (after decoding: LOB/VOL1, or RAW)
    AMNP, // seems to be AmberMoon-only
    // ONLY as file header:
    //     [hdr:32]
    //     [num_files:16]
    //     [compressedsize_1:32]
    //        ...
    //     [compressedsize_num_files:32]
    //     ... (filedata_1)  (if ZERO: JH(1)-encoded, INCLUDING the ZERO header; if VOL1/LOB: LOB-encoded)
    //        ...
    //     ... (filedata_num_files)  (if ZERO: JH(num_files)-encoded, INCLUDING the ZERO header; if VOL1/LOB: LOB-encoded)
    AMPC,
    // ONLY as file header:
    //     [hdr:32]
    //     [num_files:16]
    //     [compressedsize_1:32]
    //        ...
    //     [compressedsize_num_files:32]
    //     ... (filedata_1, LOB/VOL1 header, or RAW)
    //        ...
    //     ... (filedata_num_files, LOB/VOL1 header, or RAW)
    AMBR,
    // ONLY as file header:
    //     [hdr:32]
    //     [num_files:16]
    //     [size_1:32]
    //        ...
    //     [size_num_files:32]
    //     ... (filedata_1, no headers)
    //        ...
    //     ... (filedata_num_files, no headers)
    ZERO,  // in AMNP
    RAW,   // in AMNC, AMBR, AMPC
}

impl FileHeaderType {
    fn is_container(&self) -> bool {
	match self {
	    FileHeaderType::AMNC => true,
	    FileHeaderType::AMNP => true,
	    FileHeaderType::AMPC => true,
	    FileHeaderType::AMBR => true,
	    _                    => false,
	}
    }
}

impl fmt::Display for FileHeaderType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ================================================================================

/**
 * Big-Endian data buffer
 */
pub struct DataBuf<'a> {
    data : &'a mut [u8],
}



impl<'a> DataBuf<'a> {

    fn len(&self) -> usize {
	self.data.len()
    }

    fn u16(&self, pos : usize) -> u16 {
	return decode::u16(self.data, pos);
    }

    fn set_u16(&mut self, pos : usize, v : u16) {
	self.data[pos] = (v >> 8) as u8;
	self.data[pos + 1] = (v & 0xff) as u8;
    }

    fn u32(&self, pos : usize) -> u32 {
	return decode::u32(self.data, pos);
    }

    fn header_type(&self, offset : usize) -> FileHeaderType {
	let bytes = &self.data[offset..offset + 4];
	match bytes {
	    [0x01, b'L', b'O', b'B'] => FileHeaderType::LOB,
	    [b'A', b'M', b'N', b'C'] => FileHeaderType::AMNC,
	    [b'A', b'M', b'N', b'P'] => FileHeaderType::AMNP,
	    [b'A', b'M', b'P', b'C'] => FileHeaderType::AMPC,
	    [b'A', b'M', b'B', b'R'] => FileHeaderType::AMBR,
	    [b'J', b'H', a, b]       => FileHeaderType::JH(((a + 0) as u16) << 8 | ((b + 0) as u16)),
	    [0,    0,    0,    0   ] => FileHeaderType::ZERO,
	    _                        => FileHeaderType::RAW,
	}
    }

    fn as_vec(&self, offset : usize) -> Vec<u8> {
	let mut vec = vec![0; self.len() - offset];
	vec.copy_from_slice(&self.data[offset..]);
	return vec;
    }

    /* JH decoding */
    fn decode_jh(&mut self, offset : usize, key : u16) {
	let mut ckey = key;
	for i in (offset as usize >> 1)..(self.len() >> 1) {
	    let pos = i * 2;
	    let v = self.u16(pos);
	    self.set_u16(pos, v ^ ckey);
	    ckey = (ckey << 4) + 87;
	}
    }

    /* LOB decompression */
    fn decompress_lob(&self, start : usize, size: usize) -> Vec<u8> {
	let mut result : Vec<u8> = vec![0; size];
	let mut write_pos : usize = 0;
	let mut read_pos = start;
	let mut header = 0;
	let mut header_count = 8;
	while write_pos < size {
	    if header_count == 8 { // need new header
		header = self.data[read_pos];
		read_pos += 1;
		header_count = 0;
		ptrace!("-- decompressed-mask= {:02x}", header);
	    }
	    let next_is_compressed = (header & 0x80) == 0;
	    header_count += 1;
	    header <<= 1;
	    ptrace!("-- readpos {} writepos {}", read_pos, write_pos);
	    if next_is_compressed {
		let hi = self.data[read_pos];
		let lo = self.data[read_pos + 1] as usize;
		read_pos += 2;
		let copy_length_requested = ((hi & 0xf) + 3) as usize;
		let copy_length = min(copy_length_requested, size - write_pos);
		let copy_offset : usize = (((hi & 0xf0) as usize) << 4) | lo;
		ptrace!("-- hilo={:02x} {:02x}  -> offset={copy_offset}; len={copy_length}", hi, lo);
		let src_start = write_pos - copy_offset;
		let src_end = src_start + copy_length;
		for i in src_start..src_end { result[i + copy_offset] = result[i] }
		write_pos += copy_length;
	    } else {
		// uncompressed byte
		result[write_pos] = self.data[read_pos];
		write_pos += 1;
		read_pos += 1;
	    }
	}
	return result;
    }
}

// ----------------------------------------

#[test]
fn test_databuf_len() {
    let mut d : [u8; 3] = [ 1, 2, 3 ];
    assert_eq!(DataBuf { data : &mut d[0..0] }.len(), 0);
    assert_eq!(DataBuf { data : &mut d[0..1] }.len(), 1);
    assert_eq!(DataBuf { data : &mut d[1..1] }.len(), 0);
    assert_eq!(DataBuf { data : &mut d[1..=2] }.len(), 2);
}

#[test]
fn test_databuf_u16() {
    let mut d : [u8; 7] = [ 0, 1, 2, 3, 0xe8, 0xff, 0xff ];
    let db = DataBuf { data : &mut d };
    assert_eq!(db.u16(0), 1);
    assert_eq!(db.u16(1), 258);
    assert_eq!(db.u16(2), 515);
    assert_eq!(db.u16(3), 1000);
    assert_eq!(db.u16(5), 65535);
}

#[test]
fn test_databuf_u32() {
    let mut d : [u8; 8] = [ 0x01, 0x81, 0xfa, 0xc3, 0xd1, 0xb3, 0x97, 0x8e ];
    let db = DataBuf { data : &mut d };
    assert_eq!(db.u32(0), 0x0181fac3 );
    assert_eq!(db.u32(1), 0x81fac3d1 );
    assert_eq!(db.u32(2), 0xfac3d1b3 );
    assert_eq!(db.u32(3), 0xc3d1b397 );
    assert_eq!(db.u32(4), 0xd1b3978e );
}

#[test]
fn test_databuf_set_u16() {
    let mut d : [u8; 8] = [ 0x01, 0x81, 0xfa, 0xc3, 0xd1, 0xb3, 0x97, 0x8e ];
    let mut db = DataBuf { data : &mut d };
    db.set_u16(0, 0x3e8);
    db.set_u16(3, 0xbeef);
    assert_eq!(db.u32(0), 0x03e8fabe );
    assert_eq!(db.u32(4), 0xefb3978e );
}

#[test]
fn test_databuf_decode_jh() {
    let mut d : [u8; 8] = [ 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88 ];
    let mut db = DataBuf { data : &mut d };
    db.decode_jh(0, 0x1248);
    assert_eq!(db.u16(0), 0x036a ); // key = 0x1248
    assert_eq!(db.u16(2), 0x1793 ); // key = 0x24d7 = 0x57 + 0x2480
    assert_eq!(db.u16(4), 0x18a1 ); // key = 0x4dc7 = 0x57 + 0x4d70
    assert_eq!(db.u16(6), 0xab4f ); // key = 0xdcc7 = 0x57 + 0xdc70
}

#[test]
fn test_databuf_decompress_lob() {
    let mut d : [u8; 18] = [ 0xf4, // -- header
			     // raw
			     0x11, 0x22, 0x33, 0x44,
			     // compressed
			     0x00, 0x03, // 22 33 44
			     // raw
			     0xaa,
			     // compressed
			     0x01, 0x06, // 33 44 22 33
			     // compressed
			     0x00, 0x05, // aa 33 44

			     0xe0, // -- header
			     // raw
			     0xbb, 0xcc, 0xdd,
			     // compressed
			     0x09, 0x0d, // 33 44 aa 33 44 22 33 aa 33 44 bb cc
    ];
    let db = DataBuf { data : &mut d };
    let out = db.decompress_lob(0, 29);
    assert_eq!(out, [0x11, 0x22, 0x33, 0x44,
		     0x22, 0x33, 0x44,
		     0xaa,
		     0x33, 0x44, 0x22, 0x33,
		     0xaa, 0x33, 0x44,
		     0xbb, 0xcc, 0xdd,
		     0x33, 0x44, 0xaa, 0x33, 0x44, 0x22, 0x33, 0xaa, 0x33, 0x44, 0xbb] );
}

// ================================================================================
pub struct DataFile {
    pub filetype : FileHeaderType,
    header_offset : usize, // Can be 4 for decoded JH
    pub num_entries : u16,
    data : Vec<u8>,
}

impl DataFile {
    pub fn load(path : &Path) -> DataFile {
	let mut f = File::open(path).unwrap();
	let meta = f.metadata().unwrap();
	let mut buffer = vec![0; meta.len() as usize];
	f.read(&mut buffer).unwrap();
	let mut result = DataFile { filetype      : FileHeaderType::RAW,
				    header_offset : 0,
				    num_entries   : 0,
				    data          : Vec::from(buffer),
	};
	let buf = result.as_buf(0);
	let filetype = buf.header_type(0);
	let num_entries = if filetype.is_container() { buf.u16(4) } else { 1 };
	result.filetype = filetype;
	result.num_entries = num_entries;
	return result;
    }

    fn as_buf<'a>(&'a mut self, offset : usize) -> DataBuf<'a> {
	let slice_data : &'a mut [u8] = &mut self.data[offset..];
	let v : DataBuf<'a> = DataBuf { data : slice_data };
	return v;
    }

    /// Gets a buffer for a specific contained file, assuming a container format
    /// No decompression / decoding is performed
    fn _entry_buf<'a>(&'a mut self, index : u16) -> DataBuf<'a> {
	let size_offset : usize = 4 + 2;
	let mut entry_offset : usize = 4 * self.num_entries as usize;
	let buf = self.as_buf(self.header_offset + size_offset);
	for i in 0..index {
	    let i_size : usize = buf.u32((i * 4) as usize) as usize;
	    entry_offset += i_size;
	}
	let size : usize = buf.u32((4 * index) as usize) as usize;
	let entry_end = entry_offset + size;
	let slice_data : &'a mut [u8] = &mut buf.data[entry_offset..entry_end];
	let v : DataBuf<'a> = DataBuf { data : slice_data };
	return v;
    }

    /// Decodes and retrieves one entry in the file
    ///
    /// # Arguments
    /// * `index` - Index of the entry to decode.  Requires `index < self.num_entries`.
    pub fn decode(&mut self, index : u16) -> Vec<u8> {
	assert!(index <= self.num_entries);
	match self.filetype {
	    // JH encryption
	    FileHeaderType::JH(k) => {
		let mut buf = self.as_buf(4);
		if DEBUG {
		    pdebug!("  JH({k}) -> decoding {}", buf.len());
		}
		buf.decode_jh(0, k);
		self.filetype = buf.header_type(0);
		self.header_offset = 4;
		return self.decode(index);
	    }
	    // LOB compression
	    FileHeaderType::LOB   => {
		let size = self.as_buf(self.header_offset).u32(4) & 0xffffff;
		if DEBUG {
		    let marker = self.as_buf(self.header_offset).u32(4) >> 24;
		    pdebug!("   LOB -> decompressing to {}, marker {}", size, marker);
		}
		return self.as_buf(self.header_offset).decompress_lob(12, size as usize);
	    }
	    // AmberMoon formats
	    FileHeaderType::AMNC  => panic!("SMNC file: AmberMoon not yet supported"),
	    FileHeaderType::AMNP  => panic!("AMNP file: AmberMoon not yet supported"),
	    // AMPC: (partially) compressed
	    FileHeaderType::AMPC  => {
		let buf = self._entry_buf(index);
		match buf.header_type(0) {
		    FileHeaderType::LOB => {
			let decompressed_size = (buf.u32(4) as usize) & 0xffffff;
			if DEBUG {
			    let marker = (buf.u32(4) as usize) >> 24;
			    pdebug!("   AMPC.LOB -> decompressing to {}, marker {}", decompressed_size, marker);
			}
			return buf.decompress_lob(12, decompressed_size);
		    }
		    // Raw?
		    _ => {
			if DEBUG {
			    pdebug!("   AMPC.RAW");
			}
			return buf.as_vec(0);
		    }
		}
	    }
	    // AMBR: uncompressed, no header
	    FileHeaderType::AMBR  => {
		if DEBUG {
		    pdebug!("   AMBR.RAW");
		}
		let buf = self._entry_buf(index);
		return buf.as_vec(0);
	    }
	    // RAW / ZERO
	    _ => {
		// Not encoded in any way?
		let mut vec = vec![0; self.data.len() - self.header_offset];
		if DEBUG {
		    pdebug!("   RAW -> getting {}", vec.len());
		}
		vec.copy_from_slice(&self.data[self.header_offset..]);
		return vec;
	    }
	}
    }
}


// ----------------------------------------


fn load_relative(path : &str, filename : &str) -> DataFile {
    let fullpath = Path::new(path).join(filename);
    return DataFile::load(&fullpath);
}

pub struct AmberstarFiles {
    pub path : String,
    pub amberdev : Vec<u8>,
    pub string_fragments : string_fragment_table::StringFragmentTable,
    pub map_text : Vec<map_string_table::MapStringTable>,
    pub code_text : Vec<map_string_table::MapStringTable>,
    pub pics80 : Vec<Pixmap>,
    pub pic_intro : Pixmap,
    pub palettes : Vec<Palette>,
    pub sample_data : sampledata::SampleData,
    pub songs : Vec<Song>,
    pub tiles : Vec<Tileset<Pixmap>>,
    pub maps : Vec<Map>,
    pub bg_pictures : Vec<Vec<IndexedPixmap>>,
    pub labgfx : labgfx::LabInfo,
}

fn load_text_vec(dfile : &mut DataFile, fragments : &string_fragment_table::StringFragmentTable) -> Vec<map_string_table::MapStringTable> {
    let mut map_text = vec![];
    for i in 0..dfile.num_entries {
	let data = dfile.decode(i);
	let mst : map_string_table::MapStringTable = map_string_table::MapStringTable::new(&data[..],
											   fragments);
	map_text.push(mst);
    }
    return map_text;
}

fn load_pic80(dfile : &mut DataFile, index : u32) -> Pixmap {
    let pic_index = index as u16;
    let pal_index = (index + 1) as u16;
    let picdata = pixmap::new(&dfile.decode(pic_index), 80, 80, 4);
    let palette = palette::new(&dfile.decode(pal_index), 16);
    return picdata.with_palette(&palette);
}

fn load_palettes(dfile : &mut DataFile) -> Vec<Palette> {
    let mut result = vec![];
    for i in 0..dfile.num_entries {
	let dat = dfile.decode(i);
	let pal = palette::new_with_header(&dat[..], 0x1f);
	result.push(pal.with_transparency(0));
    }
    return result;
}

fn load_tiles(dfile : &mut DataFile) -> Vec<Tileset<Pixmap>> {
    let mut result = vec![];
    for e in 0..dfile.num_entries {
	let dat = dfile.decode(e);
	result.push(tile::new(&dat));
    }
    return result;
}

fn load_maps(dfile : &mut DataFile) -> Vec<Map> {
    // And here's the same in higher-order functional style:
    (0..dfile.num_entries).map(|i| map::new(i as usize, &dfile.decode(i))).collect()
}

impl AmberstarFiles {
    pub fn load<'a>(&self, f : &str) -> DataFile {
	return load_relative(&self.path, f);
    }

    pub fn new(path : &str) -> AmberstarFiles {
	let amberdev = load_relative(path, "AMBERDEV.UDO").decode(0);
	const STRINGTABLE_OFFSET : usize = 0x2170b;
	let string_fragments = string_fragment_table::StringFragmentTable::new(&amberdev[STRINGTABLE_OFFSET..]);

	let map_text = load_text_vec(&mut load_relative(path, "MAPTEXT.AMB"), &string_fragments);
	let code_text = load_text_vec(&mut load_relative(path, "CODETXT.AMB"), &string_fragments);

	let mut pics80_f = load_relative(path, "PICS80.AMB");
	let mut pics80 = vec![];
	for i in 0..(pics80_f.num_entries >> 1) {
	    pics80.push(load_pic80(&mut pics80_f, (i as u32) << 1));
	}

	let mut pall_f = load_relative(path, "COL_PALL.AMB");
	let palettes = load_palettes(&mut pall_f);

	let mut intro_f = load_relative(path, "INTRO_P.UDO");
	let pic_intro_raw = pixmap::new(&intro_f.decode(0)[82964..], 320, 200, 4);
	let pic_intro = pic_intro_raw.with_palette(&palettes[0]);

	let mut songseeker = music::seeker(&amberdev, 0x4cd00);
	let mut songs = vec![];
	while let Some(song) = songseeker.next() {
	    songs.push(song);
	}

	let mut sampledata_f = load_relative(path, "SAMPLEDA.IMG");
	let sample_data = sampledata::SampleData::new(sampledata_f.decode(0));

	let mut tiles_f = load_relative(path, "ICON_DAT.AMB");
	let tiles = load_tiles(&mut tiles_f);

	let mut map_data_f = load_relative(path, "MAP_DATA.AMB");
	let maps = load_maps(&mut map_data_f);

	let mut labblock_f = load_relative(path, "LABBLOCK.AMB");
	let mut lab_data_f = load_relative(path, "LAB_DATA.AMB");
	let labgfx = labgfx::LabInfo::load(&mut labblock_f, &mut lab_data_f);

	let mut bg_pictures_f = load_relative(path, "BACKGRND.AMB");
	let bg_pictures = pictures::load_backgrounds(&mut bg_pictures_f);

	let path : String = format!("{}", path);

	return AmberstarFiles {
	    path,
	    amberdev,
	    string_fragments,
	    map_text,
	    code_text,
	    pics80,
	    pic_intro,
	    palettes,
	    sample_data,
	    songs,
	    tiles,
	    maps,
	    bg_pictures,
	    labgfx,
	}
    }
}
