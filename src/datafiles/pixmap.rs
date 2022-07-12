// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use log::debug;
use sdl2::{render::{Texture, TextureCreator}, pixels::PixelFormatEnum};

use crate::datafiles::decode;
use crate::datafiles::palette::Palette;

/// An indexed pixel map without a palette
pub struct IndexedPixmap {
    pub width : usize,
    pub height : usize,
    pub pixels : Vec<u8>,
}

pub fn new(src : &[u8], width : usize, height : usize, bitplanes : usize) -> IndexedPixmap {
    let mut result = IndexedPixmap{
	width,
	height,
	pixels : vec![0; (width * height) as usize],
    };
    let last_word_len = if width & 0xf == 0 { 16 } else { width & 0xf };
    let words_per_line = ((width + 15) >> 4) as usize;
    for y in 0..height {
	let line_pos = 2 * (words_per_line * bitplanes) * y;
	for xword_index in 0..words_per_line {
	    for bp in 0..bitplanes {
		let bitplane_value = 1 << bp;
		let pos = line_pos + (xword_index* bitplanes + bp) * 2;
		let mut mask = decode::u16(src, pos);

		let word_len = if xword_index + 1 < words_per_line { 16 } else { last_word_len };
		for xrel in 0..word_len {
		    let x = ((xword_index * 16) + xrel) as usize;
		    if (mask & 0x8000) == 0x8000 {
			let out_pos = (y * width + x) as usize;
			if y < 2 {
			    debug!("  {pos}[{x},{y}] -> {out_pos} |= {bitplane_value}")
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

fn icon_header(src : &[u8]) -> (usize, usize, usize, usize) {
    let width = 1 + decode::u16(src, 0) as usize;
    let height = 1 + decode::u16(src, 2) as usize;
    let bitplanes = decode::u16(src, 4) as usize;
    let width_words = ((width + 15) >> 4) * 2;
    return (width, height, bitplanes, width_words);
}

/// Image with width, height, #bitplanes header.  Also returns # of bytes used.
pub fn new_icon_frame(src : &[u8]) -> IndexedPixmap {
    const HEADER_SIZE : usize = 6;

    let (width, height, bitplanes, width_words) = icon_header(&src);

    return new(&src[HEADER_SIZE..], width, height, bitplanes);
}

pub fn icon_len(src : &[u8]) -> usize {
    const HEADER_SIZE : usize = 6;

    let (width, height, bitplanes, width_words) = icon_header(&src);

    print!("  -> {:x}, {:x}, {:x}", width, height, bitplanes);

    let size = HEADER_SIZE + (width_words * height * bitplanes);
    println!("  => {size:x}");

    return size;
}

impl IndexedPixmap {
    pub fn with_palette(&self, palette : &Palette) -> Pixmap {
	let mut data : Vec<u8> = vec![0; self.pixels.len() * 4];
	let mut pos = 0;
	for pal_index in self.pixels.iter() {
	    let col = palette.get((*pal_index & 0xf) as usize);
	    /* Since we can't convert u32 to u8 vectors, we here force endianness to be little */
	    data[pos + 3] = col.r;
	    data[pos + 2] = col.g;
	    data[pos + 1] = col.b;
	    data[pos + 0] = col.a;
	    pos += 4;
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
    pub width : usize,
    pub height : usize,
    pub data : Vec<u8>,
}

impl Pixmap {
    pub fn as_texture<'a, T>(&self, tc : &'a TextureCreator<T>) -> Texture<'a> {
	let mut texture = tc.create_texture_static(PixelFormatEnum::RGBA8888, self.width as u32, self.height as u32).unwrap();
	texture.update(None, &self.data[..], (self.width * 4) as usize).unwrap();
	return texture;
    }
}

