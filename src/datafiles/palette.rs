// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use sdl2::pixels::Color;
use crate::datafiles::decode;

#[derive(Clone,Debug)]
pub struct Palette {
    pub colors : Vec<Color>,
}

const AMBERDEV_PALETTE_OFFSETS : [usize; 1/*25*/] = [
    0x20f70,
    // 0x210f8,
    // 0x2113e,
    // 0x29026,
    // 0x292a2,
    // 0x292c4,
    // 0x2930a,
    // 0x29322 - (32 - 8),
    // 0x29372,
    // 0x293b8,
    // 0x2987e,
    // 0x29a0a,
    // 0x29a2c,
    // 0x29a4c,
    // 0x29b38,
    // 0x29d90,
    // 0x29df8,
    // 0x29e3e,
    // 0x31300,
    // 0x31322,
    // 0x31848,
    // 0x3188e,
    // 0x31f62,
    // 0x323dc,
    // 0x32858,
    ];

lazy_static! {
// EGA palette for testing colour indices
    pub static ref TEST_PALETTE : Palette = Palette {
	colors : vec![
	    Color{ r : 0x00, g: 0x00, b: 0x00, a : 0xff }, // 0 black
	    Color{ r : 0x00, g: 0x00, b: 0xaa, a : 0xff }, // 1 blue
	    Color{ r : 0x00, g: 0xaa, b: 0x00, a : 0xff }, // 2 green
	    Color{ r : 0x00, g: 0xaa, b: 0xaa, a : 0xff }, // 3 cyan
	    Color{ r : 0xaa, g: 0x00, b: 0x00, a : 0xff }, // 4 red
	    Color{ r : 0xaa, g: 0x00, b: 0xaa, a : 0xff }, // 5 purple
	    Color{ r : 0xaa, g: 0x55, b: 0x00, a : 0xff }, // 6 brown
	    Color{ r : 0xaa, g: 0xaa, b: 0xaa, a : 0xff }, // 7 light grey
	    Color{ r : 0x55, g: 0x55, b: 0x55, a : 0xff }, // 8 dark grey
	    Color{ r : 0x55, g: 0x55, b: 0xff, a : 0xff }, // 9 light blue
	    Color{ r : 0x55, g: 0xff, b: 0x55, a : 0xff }, // a light green
	    Color{ r : 0x55, g: 0xff, b: 0xff, a : 0xff }, // b light cyan
	    Color{ r : 0xff, g: 0x55, b: 0x55, a : 0xff }, // c light red
	    Color{ r : 0xff, g: 0x55, b: 0xff, a : 0xff }, // d light purple
	    Color{ r : 0xff, g: 0xff, b: 0x55, a : 0xff }, // e yellow
	    Color{ r : 0xff, g: 0xff, b: 0xff, a : 0xff }, // f white
	],
    };

    // pub static ref COMBAT_PALETTE : Palette = Palette {
    // 	colors : vec![
    // 	    Color{ r : 0xff, g: 0xff, b: 0xff, a : 0xff },
    // 	    Color{ r : 0x00, g: 0x00, b: 0x00, a : 0xff },
    // 	    Color{ r : 0x82, g: 0x82, b: 0x82, a : 0xff },
    // 	    Color{ r : 0x61, g: 0x61, b: 0x41, a : 0xff },
    // 	    Color{ r : 0x20, g: 0x41, b: 0x41, a : 0xff }, // 204141
    // 	    Color{ r : 0xa2, g: 0xa2, b: 0x41, a : 0xff }, // a2a241
    // 	    Color{ r : 0xa2, g: 0x61, b: 0x20, a : 0xff }, // a26120
    // 	    Color{ r : 0x82, g: 0x41, b: 0x00, a : 0xff }, // 824100
    // 	    Color{ r : 0x61, g: 0x20, b: 0x00, a : 0xff }, // 612000
    // 	    Color{ r : 0x41, g: 0xa2, b: 0x00, a : 0xff }, // 41a200
    // 	    Color{ r : 0x00, g: 0x61, b: 0x20, a : 0xff }, // 006120
    // 	    Color{ r : 0x41, g: 0x00, b: 0x00, a : 0xff }, // 410000
    // 	    Color{ r : 0x24, g: 0x6d, b: 0x92, a : 0xff }, // 246d92
    // 	    Color{ r : 0x00, g: 0x24, b: 0x49, a : 0xff }, // 002449
    // 	    Color{ r : 0x92, g: 0xb6, b: 0xb6, a : 0xff }, // 92b6b6
    // 	    Color{ r : 0xc3, g: 0xc3, b: 0xc3, a : 0xff }, // c3c3c3
    // 	],
    // };
}

// packed 0RGB format
pub fn new(src : &[u8], num_colors : usize) -> Palette {
    let mut p = Palette { colors : vec![] };
    for i in 0..num_colors {
	let r = src[i * 2] & 0x7;
	let gb = src[i * 2 + 1] & 0x77;
	let g = gb >> 4;
	let b = gb & 0xf;
	let c = Color {
	    r : r * 0x20,
	    g : g * 0x20,
	    b : b * 0x20,
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
	//let a = 6-src[(2 + i * 4) as usize];
	let r = src[(3 + i * 4) as usize];
	let g = src[(4 + i * 4) as usize];
	let b = src[(5 + i * 4) as usize];
	let c = Color {
	    r : r * factor, // max value 6 -> 252,
	    g : g * factor,
	    b : b * factor,
	    a : 0xff,
	};
	p.colors.push(c);
    }
    return p;
}

impl Palette {
    pub fn get(&self, index : usize) -> Color {
	return self.colors[index];
    }

    pub fn amberdev_palettes(data: &[u8]) -> Vec<Palette> {
	// AMBERDEV_PALETTE_OFFSETS
	//     .iter()
	//     .map(|offset|
	// 	 new(&data[(*offset)..], 16))
	//     .collect()
	let offset = AMBERDEV_PALETTE_OFFSETS[0];
	let pal = new(&data[offset..], 16);
	print!("================!!+======================\n");
	print!("Loading pal from {offset:x}: {:x?} -> {:?}\n",
	       &data[offset..(offset+32)],
	       &pal);
	return vec![pal];
    }

    pub fn with_transparency(&self, index : usize) -> Palette {
	let mut colors = self.colors.clone();
	colors[index].a = 0x00;
	colors[index].r = 0x00;
	colors[index].g = 0xff;
	colors[index].b = 0x00;
	return Palette {
	    colors,
	}
    }
}
