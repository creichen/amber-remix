// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use sdl2::{pixels::Color, render::{TextureCreator, Texture, BlendMode, Canvas, RenderTarget}, rect::Rect};
use crate::datafiles::{palette, decode, pixmap};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::pixmap::Pixmap;

// ----------------------------------------
// TileIcons are animated graphics used for map tiles.

pub struct Tileset<T> {
    pub tile_icons : Vec<TileIcon<T>>,
    pub player_icon_index : usize,
}

pub struct TileIcon<T> {
    pub frames : Vec<T>,
    pub magic_flags : u32,
    pub map_color : Color,
}

const PALETTE_BRIGHTNESS : u8 = 255 / 7; /* VALIDATE ME */
const ANIM_TYPE_OFFSET : usize = 0x2;
const PALETTE_SIZE : usize = 0x42; /* Assuming 16 entries */
const COLOR_INDEX_FOR_TRANSPARENCY : usize = 0;

pub fn new(src: &[u8]) -> Tileset<Pixmap> {
    let mut tile_icons = vec![];
    let num_icons = src[ANIM_TYPE_OFFSET..].iter().position(|x|  *x == 0); // Always 250, I think?
    let player_icon_index = decode::u16(src, 0) as usize;

    assert_eq!(num_icons, Some(250), "num_icons != 250 is possible, but I haven't observed it anywhere");
    if let Some(num_icons) = num_icons {
	let palette_offset = num_icons * 8; // anim_type(u8), anim_start(u16), magic_flags1(u32), magic_flags2(u8)
	assert!(src.len() >= palette_offset + PALETTE_SIZE);
	let base = &src[2..];
	let palette = palette::new_with_header(&base[palette_offset..], PALETTE_BRIGHTNESS).with_transparency(COLOR_INDEX_FOR_TRANSPARENCY);
	let anim_start_base = &base[num_icons * 1..];
	let magic_flags1_base = &base[num_icons * 3..];
	let magic_flags2_base = &base[num_icons * 7..];

	let mut frame_start = vec![0];
	let frame_base = &base[palette_offset + PALETTE_SIZE..];
	let mut frame_pos = 0;
	while frame_pos + 6 < frame_base.len() {
	    let image_len = pixmap::icon_len(&frame_base[frame_pos..]);
	    println!("Frame {} @ {:x}", frame_start.len(), 2 + palette_offset + PALETTE_SIZE + frame_pos);
	    frame_start.push(frame_pos);
	    frame_pos += image_len;
	}

	for i in 0..num_icons {
	    println!("Icon {i} of {num_icons}");
	    let num_frames = base[i] as usize;
	    let anim_start = decode::u16(&anim_start_base, i * 2) as usize;
	    let anim_end = anim_start + num_frames;
	    println!("  {anim_start}..{anim_end}");
	    let magic_flags = decode::u32(&magic_flags1_base, i * 4);
	    let map_color_index = magic_flags2_base[i];

	    let mut frames = vec![];
	    for image_index in anim_start..anim_end {
		// if Some(img) = images.
		let pos = frame_start[image_index];
		println!("  +{image_index}@{:x}", 2 + pos + palette_offset + PALETTE_SIZE);
		let frame = pixmap::new_icon_frame(&frame_base[pos..]);
		let frame = frame.with_palette(&palette);
		frames.push(frame);
	    }
	    tile_icons.push(TileIcon {
		frames,
		magic_flags,
		map_color : palette.get(map_color_index as usize),
	    });
	}
	return Tileset {
	    tile_icons,
	    player_icon_index,
	}
    }
    panic!("Could not determine number of tileset icons");
}

// ----------------------------------------
// TileTextures

impl Tileset<Pixmap> {
    pub fn as_textures<'a, T>(&self, tc: &'a TextureCreator<T>) -> Tileset<Texture<'a>> {
	let mut icons = vec![];
	for t in &self.tile_icons[..] {
	    let mut frames = vec![];
	    for pixmap in &t.frames {
		let mut texture = pixmap.as_texture(tc);
		texture.set_blend_mode(BlendMode::Blend);
	     	frames.push(texture);
	    }
	    icons.push(TileIcon {
		frames,
		magic_flags : t.magic_flags,
		map_color : t.map_color,
	    });
	}
	return Tileset {
	    tile_icons : icons,
	    player_icon_index : self.player_icon_index,
	}
    }
}

const TILE_SIZE : u32 = 16;

impl<'a> Tileset<Texture<'a>> {
    pub fn draw<T>(&self, canvas : &mut Canvas<T>, tile_index : usize,
		   x : isize, y : isize, tick : usize) where T : RenderTarget {
	self.draw_resize(canvas, tile_index, x, y, tick, 1);
    }

    pub fn draw_resize<T>(&self, canvas : &mut Canvas<T>, tile_index : usize,
		   x : isize, y : isize, tick : usize, scale : usize)  where T : RenderTarget {
	if tile_index > 0 {
	    let tile = &self.tile_icons[tile_index - 1];
	    let texture = &tile.frames[tick & tile.frames.len()];
	    canvas.copy(texture,
			Rect::new(0, 0, TILE_SIZE, TILE_SIZE),
			Rect::new(x as i32, y as i32, TILE_SIZE * (scale as u32), TILE_SIZE * (scale as u32))).unwrap();
	}
    }
}
