// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use crate::datafiles::{palette, decode, pixmap};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::pixmap::Pixmap;

// ----------------------------------------
// TileIcons are animated graphics used for map tiles.

pub struct Tileset {
    pub tile_icons : Vec<TileIcon>,
    pub player_icon_index : usize,
}

pub struct TileIcon {
    pub frames : Vec<Pixmap>,
    magic_flags1 : u32,
    magic_flags2 : u8,
}

const PALETTE_BRIGHTNESS : u8 = 255 / 7; /* VALIDATE ME */
const ANIM_TYPE_OFFSET : usize = 0x2;
const PALETTE_SIZE : usize = 0x42; /* Assuming 16 entries */
const COLOR_INDEX_FOR_TRANSPARENCY : usize = 0;

pub fn new(src: &[u8]) -> Tileset {
    let mut tile_icons = vec![];
    let num_icons = src[ANIM_TYPE_OFFSET..].iter().position(|x|  *x == 0); // Always 250, I think?
    let player_icon_index = decode::u16(src, 0) as usize;

    assert_eq!(num_icons, Some(250), "num_icons != 250 is possible, but I haven't observed it anywhere");
    if let Some(num_icons) = num_icons {
	let palette_offset = num_icons * 8; // anim_type(u8), anim_start(u16), magic_flags1(u32), magic_flags2(u8)
	assert!(src.len() >= palette_offset + PALETTE_SIZE);
	let palette = palette::new_with_header(&src[palette_offset..], PALETTE_BRIGHTNESS);
	let frames_num_base = &src[2..];
	let anim_start_base = &src[ANIM_TYPE_OFFSET+(num_icons * 1)..];
	let magic_flags1_base = &src[ANIM_TYPE_OFFSET+(num_icons * 3)..];
	let magic_flags2_base = &src[ANIM_TYPE_OFFSET+(num_icons * 7)..];

	let mut frame_start = vec![];
	let frame_base = &src[palette_offset + PALETTE_SIZE..];
	let mut frame_pos = 0;
	let mut frame_index = 1;
	while frame_pos + 6 < frame_base.len() {
	    let image_len = pixmap::icon_len(&frame_base[frame_pos..]);
	    frame_start[frame_index] = image_len;
	    frame_pos += image_len;
	    frame_index += 1;
	}

	for i in 0..num_icons {
	    let num_frames = frames_num_base[i] as usize;
	    let anim_start = decode::u16(&anim_start_base, i * 2) as usize;
	    let anim_end = anim_start + num_frames;
	    let magic_flags1 = decode::u32(&magic_flags1_base, i * 4);
	    let magic_flags2 = magic_flags2_base[i];

	    let mut frames = vec![];
	    for image_index in anim_start..anim_end {
		// if Some(img) = images.
		let pos = frame_start[image_index];
		let frame = pixmap::new_icon_frame(&frame_base[pos..]);
		let frame = frame.with_palette(&palette);
		frames.push(frame);
	    }
	    tile_icons.push(TileIcon {
		frames,
		magic_flags1,
		magic_flags2,
	    });
	}
	return Tileset {
	    tile_icons,
	    player_icon_index,
	}
    }
    panic!("Could not determine number of tileset icons");
}
