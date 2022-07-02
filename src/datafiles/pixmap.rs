use sdl2::{render::{Texture, TextureCreator}, pixels::PixelFormatEnum};

use crate::datafiles::decode;
use crate::datafiles::palette::Palette;

/// An indexed pixel map without a palette
pub struct IndexedPixmap {
    width : u32,
    height : u32,
    pixels : Vec<u8>,
}

// Seems to be the AmberMoon format?
pub fn new_ambermoon(src : &[u8], width : u32, height : u32, bitplanes : u32) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_byte_len = if width & 0x7 == 0 { 8 } else { width & 0x7 };
    let bytes_per_bp_line = ((width + 7) >> 3) as usize;
    let bytes_per_line = ((bytes_per_bp_line as u32) * bitplanes) as usize;
    for y in 0..height {
	for bp in 0..bitplanes {
	    let in_pos = (bytes_per_line as u32 * y
	     		  + bytes_per_bp_line as u32 * bp) as usize;
	    // let in_pos = ((bytes_per_bp_line as u32 * height) as u32 * bp
	    // 		  + bytes_per_bp_line as u32 * y) as usize;
	    let line_in = &src[in_pos..in_pos + bytes_per_bp_line];
	    let out_pos = (y * width) as usize;
	    let out_pos_end = (out_pos as u32 + width) as usize;
	    let line_out = &mut result.pixels[out_pos..out_pos_end];
	    let bitplane_value = 1 << bp;
	    for x_byte in 0..bytes_per_bp_line {
		let mut mask_byte : u8 = line_in[x_byte];
		let byte_len = if x_byte + 1 < bytes_per_bp_line { 8 } else { last_byte_len };
		for xrel in 0..byte_len {
		    let x = ((x_byte as u32 * 8) + xrel) as usize;
		    if (mask_byte & 0x80) == 0x80 {
			line_out[x] |= bitplane_value;
		    }
		    mask_byte <<= 1
		}
	    }
	}
    }
    return result;
}

// okayish
pub fn new16(src : &[u8], width : u32, height : u32, bitplanes : u32) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_word_len = if width & 0xf == 0 { 16 } else { width & 0xf };
    let words_per_line = ((width + 15) >> 4) as usize;
    for y in 0..height {
	let line_pos = (words_per_line as u32 * bitplanes) * y;
	for xword_index in 0..words_per_line {
	    for bp in 0..bitplanes {
		let bitplane_value = 1 << bp;

		let mut mask = decode::u16(src, (line_pos + (xword_index as u32 * bitplanes as u32 + bp as u32) as u32 * 2) as usize);

		let word_len = if xword_index + 1 < words_per_line { 16 } else { last_word_len };
		for xrel in 0..word_len {
		    let x = ((xword_index as u32 * 16) + xrel) as usize;
		    if (mask & 0x8000) == 0x8000 {
			result.pixels[(y * width as u32 + x as u32) as usize] |= bitplane_value;
		    }
		    mask <<= 1
		}
	    }
	}
    }
    return result;
}

// bad
pub fn new8(src : &[u8], width : u32, height : u32, bitplanes : u32) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_word_len = if width & 0x7 == 0 { 8 } else { width & 0x7 };
    let words_per_line = ((width + 7) >> 3) as usize;
    for y in 0..height {
	let line_pos = (words_per_line as u32 * bitplanes) * y;
	for xword_index in 0..words_per_line {
	    for bp in 0..bitplanes {
		let bitplane_value = 1 << bp;

		let mut mask = src[(line_pos + (xword_index as u32 * bitplanes as u32 + bp as u32) as u32) as usize];

		let word_len = if xword_index + 1 < words_per_line { 8 } else { last_word_len };
		for xrel in 0..word_len {
		    let x = ((xword_index as u32 * 8) + xrel) as usize;
		    if (mask & 0x80) == 0x80 {
			result.pixels[(y * width as u32 + x as u32) as usize] |= bitplane_value;
		    }
		    mask <<= 1
		}
	    }
	}
    }
    return result;
}

// bad
pub fn new32(src : &[u8], width : u32, height : u32, bitplanes : u32) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_word_len = if width & 0x1f == 0 { 32 } else { width & 0x1f };
    let words_per_line = ((width + 31) >> 5) as usize;
    for y in 0..height {
	let line_pos = (words_per_line as u32 * bitplanes) * y;
	for xword_index in 0..words_per_line {
	    for bp in 0..bitplanes {
		let bitplane_value = 1 << bp;

		let mut mask = decode::u32(src, (line_pos + (xword_index as u32 * bitplanes as u32 + bp as u32) as u32 * 4) as usize);

		let word_len = if xword_index + 1 < words_per_line { 32 } else { last_word_len };
		for xrel in 0..word_len {
		    let x = ((xword_index as u32 * 32) + xrel) as usize;
		    if (mask & 0x80000000) == 0x80000000 {
			result.pixels[(y * width as u32 + x as u32) as usize] |= bitplane_value;
		    }
		    mask <<= 1
		}
	    }
	}
    }
    return result;
}

pub fn new(src : &[u8], width : u32, height : u32, bitplanes : u32) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_word_len = if width & 0xf == 0 { 16 } else { width & 0xf };
    let words_per_line = ((width + 15) >> 4) as usize;
    for y in 0..height {
	let line_pos = 2 * (words_per_line as u32 * bitplanes) * y;
	for xword_index in 0..words_per_line {
	    for bp in 0..bitplanes {
		let bitplane_value = 1 << bp;
		let pos = (line_pos + (xword_index as u32 * bitplanes as u32 + bp as u32) as u32 * 2) as usize;
		let mut mask = decode::u16(src, pos);

		let word_len = if xword_index + 1 < words_per_line { 16 } else { last_word_len };
		for xrel in 0..word_len {
		    let x = ((xword_index as u32 * 16) + xrel) as usize;
		    if (mask & 0x8000) == 0x8000 {
			let out_pos = (y * width as u32 + x as u32) as usize;
			if y < 2 {
			    println!("  {pos}[{x},{y}] -> {out_pos} |= {bitplane_value}")
			}
			result.pixels[out_pos] |= bitplane_value;
		    }
		    mask <<= 1
		}
	    }
	}
    }
    return result;
}

impl IndexedPixmap {
    pub fn with_palette(&self, palette : &Palette) -> Pixmap {
	let mut data : Vec<u8> = vec![0; self.pixels.len() * 4];
	let mut pos = 0;
	println!("");
	for pal_index in self.pixels.iter() {
	    let col = palette.colors[(*pal_index & 0xf) as usize];
	    /* Since we can't convert u32 to u8 vectors, we here force endianness to be little */
	    data[pos + 3] = col.r;
	    data[pos + 2] = col.g;
	    data[pos + 1] = col.b;
	    data[pos + 0] = col.a;
	    pos += 4;
	    print!("{:x}", *pal_index);
	    //print!("{:x}{:x}{:x}", col.r & 0xf, col.g & 0xf, col.b & 0xf);
	    if pos % 320 == 0 {
		println!("");
	    }
	}
	return Pixmap {
	    width : self.width,
	    height : self.height,
	    data,
	}
    }
}

// ================================================================================

pub struct Pixmap {
    pub width : u32,
    pub height : u32,
    pub data : Vec<u8>,
}

impl Pixmap {
    pub fn as_texture<'a, T>(&self, tc : &'a TextureCreator<T>) -> Texture<'a> {
	let mut texture = tc.create_texture_static(PixelFormatEnum::RGBA8888, self.width, self.height).unwrap();
	texture.update(None, &self.data[..], (self.width * 4) as usize).unwrap();
	return texture;
    }
}

