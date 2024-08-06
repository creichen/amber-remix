// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use sdl2::pixels::Color;
use crate::datafiles::decode;

use super::amberdev::Amberdev;

#[derive(Clone,Debug)]
pub struct Palette {
    pub colors : Vec<Color>,
}

pub struct DaylightGradientPalettes {
    pub day: Palette,
    pub night: Palette,
    pub twilight: Palette,
}

const AMBERDEV_PALETTE_OFFSETS : [usize; 27] = [
    0x31eda,
    0x31f62,
    0x20f70,
    0x210f8,
    0x2113e,
    0x29026,
    0x292a2,
    0x292c4,
    0x2930a,
    0x29322 - (32 - 8),
    0x29372,
    0x293b8,
    0x2987e,
    0x29a0a,
    0x29a2c,
    0x29a4c,
    0x29b38,
    0x29d90,
    0x29df8,
    0x29e3e,
    0x31300,
    0x31322,
    0x31848,
    0x3188e,
    0x31f62,
    0x323dc,
    0x32858,
    ];

// 00031edc

// packed 0RGB format
pub fn new(src : &[u8], num_colors : usize) -> Palette {
    return Palette {
	colors: colors_compressed(num_colors as usize, src),
    }
}

fn colors(num_colors: usize, src: &[u8], factor: u8) -> Vec<Color> {
    let mut result = vec![];
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
	result.push(c);
    }
    return result;
}

fn colors_compressed(num_colors: usize, src: &[u8]) -> Vec<Color> {
    let mut result = vec![];
    for i in 0..num_colors {
	//let a = 6-src[(2 + i * 4) as usize];
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
	result.push(c);
    }
    return result;
}

// Different format:
// [num : 16] [0A 0R 0G 0B], with values from 0-6
pub fn new_with_header(src : &[u8], factor : u8) -> Palette {
    let num_colors = decode::u16(src, 0);
    return Palette {
	colors: colors(num_colors as usize, src, factor),
    }
}

impl Palette {
    pub fn get(&self, index : usize) -> Color {
	return self.colors[index];
    }

    pub fn fill(col: &Color, num: usize) -> Self {
	Palette { colors: vec![*col; num] }
    }

    pub fn len(&self) -> usize { self.colors.len() }

    pub fn copy_into(&mut self, other: &Palette) {
	assert!(self.len() == other.len());
	self.colors.copy_from_slice(&other.colors);
    }

    pub fn blend_into(&mut self, other: &Palette, factor: u8) {
	assert!(self.len() == other.len());
	let factor = factor as usize;
	for (i, c) in self.colors.iter_mut().enumerate() {
	    c.r = usize::min(0xff,
			     c.r as usize + ((other.colors[i].r as usize * factor + 128) >> 8)) as u8;
	    c.g = usize::min(0xff,
			     c.g as usize + ((other.colors[i].g as usize * factor + 128) >> 8)) as u8;
	    c.b = usize::min(0xff,
			     c.b as usize + ((other.colors[i].b as usize * factor + 128) >> 8)) as u8;
	}
    }

    pub const AMBERDEV_COMBAT_PALETTES_NR: usize = 14;

    /// Day, Night, Dawn/Dusk
    pub fn daylight_palettes(amberdev: &Amberdev) -> DaylightGradientPalettes {
	const COLORS_NUM: usize = 83;
	const OFFSET: usize = COLORS_NUM * 2;
	let day_offset = amberdev.positions.daylight_tables;
	let night_offset = day_offset + OFFSET;
	let twilight_offset = night_offset + OFFSET;

	DaylightGradientPalettes {
	    day: new(&amberdev[day_offset..day_offset+OFFSET], COLORS_NUM),
	    night: new(&amberdev[night_offset..night_offset+OFFSET], COLORS_NUM),
	    twilight: new(&amberdev[twilight_offset..twilight_offset+OFFSET], COLORS_NUM),
	}
    }

    pub fn amberdev_combat_palette(amberdev: &Amberdev, index: usize) -> Palette {
	let mut p = new(amberdev.combat_palette(), 16);
	p = p.replacing(0x0c, 3, &amberdev.combat_palette_specialisation_table()[index*6..]);
	return p;
    }

    pub fn replacing(&self, first_color: usize, num: usize, new_rgb: &[u8]) -> Palette {
	let mut pal_colors = self.colors[..first_color].to_vec();
	let mut new_colors = colors_compressed(num, new_rgb);
	pal_colors.append(&mut new_colors);
	pal_colors.append(&mut self.colors[first_color+num..].to_vec());
	return Palette {
	    colors: pal_colors,
	}
    }

    pub fn amberdev_palettes(data: &[u8]) -> Vec<Palette> {
	AMBERDEV_PALETTE_OFFSETS
	    .iter()
	    .map(|offset|
		 new(&data[(*offset)..], 16))
	    .collect()
	// let offset = AMBERDEV_PALETTE_OFFSETS[0];
	// let pal = new(&data[offset..], 16);
	// print!("================!!+======================\n");
	// print!("Loading pal from {offset:x}: {:x?} -> {:?}\n",
	//        &data[offset..(offset+32)],
	//        &pal);
	// return vec![pal];
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

    pub const TEST_PALETTE_COLORS: [Color; 16] = [
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
    ];

    /// EGA-style test palette to quickly inspect colour index values
    pub fn test_palette() -> Self {
	Self { colors: Self::TEST_PALETTE_COLORS.to_vec(), }
    }
}
