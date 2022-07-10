// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use sdl2::pixels::Color;
use crate::datafiles::decode;

#[derive(Clone)]
pub struct Palette {
    pub colors : Vec<Color>,
}

// packed 0RGB format
pub fn new(src : &[u8], num_colors : usize) -> Palette {
    let mut p = Palette { colors : vec![] };
    for i in 0..num_colors {
	let r = src[i * 2] & 0xf;
	let gb = src[i * 2 + 1];
	let g = gb >> 4;
	let b = gb & 0xf;
	let c = Color {
	    r : r | (r << 4),
	    g : g | (g << 4),
	    b : b | (b << 4),
	    a : 0xff,
	};
	p.colors.push(c);
    };
    return p;
}

// Different format:
// [num : 16] [0A 0R 0G 0B], with values from 0-6
pub fn new_with_header(src : &[u8], factor : u8) -> Palette {
    let num_colors = decode::u16(src, 0);
    let mut p = Palette { colors : vec![] };
    for i in 0..num_colors {
	let a = 6-src[(2 + i * 4) as usize];
	let r = src[(3 + i * 4) as usize];
	let g = src[(4 + i * 4) as usize];
	let b = src[(5 + i * 4) as usize];
	let c = Color {
	    r : r * factor, // max value 6 -> 252,
	    g : g * factor,
	    b : b * factor,
	    a : a * factor
	};
	p.colors.push(c);
    }
    return p;
}

impl Palette {
    pub fn get(&self, index : usize) -> Color {
	return self.colors[index];
    }
    pub fn with_transparency(&self, index : usize) -> Palette {
	let mut colors = self.colors.clone();
	colors[index].a = 0x00;
	return Palette {
	    colors,
	}
    }
}
