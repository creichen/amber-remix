// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{DataFile, pixmap::{IndexedPixmap, Pixmap}, palette::Palette};
use sdl2::render::{Texture, TextureCreator};
use crate::datafiles::{decode, pixmap};

/// Single "3D perspective" image
pub struct LabPixmap<T> {
    pub xoffset : usize,
    pub yoffset : usize,
    pub pixmap : T,
}

impl<T : Clone> Clone for LabPixmap<T> {
    fn clone(&self) -> Self {
        LabPixmap {
	    xoffset : self.xoffset,
	    yoffset : self.yoffset,
	    pixmap : self.pixmap.clone(),
	}
    }
}

impl<T> LabPixmap<T> {
    fn map<U, F : Fn(&T) -> U>(&self, f : &F) -> LabPixmap<U> {
	LabPixmap {
	    xoffset : self.xoffset,
	    yoffset : self.yoffset,
	    pixmap : f(&self.pixmap),
	}
    }
}

/// Animation of several 3D perspectice images
pub struct LabImage<T> {
    /// If not-None, the image is composed of both the base_pixmap and the pixmap for the current loop.
    pub base_pixmap : Option<LabPixmap<T>>,
    pub pixmaps : Vec<LabPixmap<T>>,
}

impl LabImage<IndexedPixmap> {
    pub fn flatten(&self) -> LabImage<IndexedPixmap> {
	// WIP: broken!
	let pixmaps = match &self.base_pixmap {
	    // TODO: Some pointless copying
	    None       => {pwarn!("NO flatten"); self.pixmaps.clone()},
	    Some(base) => {pwarn!("do flatten, YES"); self.pixmaps.iter().map(|pm| LabPixmap {
		xoffset : base.xoffset,
		yoffset : base.yoffset,
		pixmap : base.pixmap.resize_and_blit(&pm.pixmap,
						     // WIP: this is very wrong!
						     if base.xoffset > pm.xoffset {todo!(); 0} else {pm.xoffset - base.xoffset},
						     if base.yoffset > pm.yoffset {todo!(); 0} else {pm.yoffset - base.yoffset}) } ).collect()},
	};
	// pdebug!("    pixmaps: {}", self.pixmaps.len());
	// for p in &self.pixmaps {
	//     pdebug!("    - {} x {}", p.pixmap.width, p.pixmap.height);
	//     }
	return LabImage {
	    base_pixmap : None,
	    //pixmaps,
	    pixmaps,
	}
    }
}

impl<T> LabImage<T> {
    fn map<U, F : Fn(&T) -> U>(&self, f : &F) -> LabImage<U> {
	LabImage {
	    base_pixmap : self.base_pixmap.as_ref().map(|i : &LabPixmap<T>| i.map(f)),
	    pixmaps : self.pixmaps.iter().map(|i : &LabPixmap<T>| i.map(f)).collect(),
	}
    }
}

#[derive(Debug)]
#[derive(Clone)]
pub enum LabBlockType {
    // Determines the number of images and how to draw them
    Error,

    // "Block" and "Decoration" use the following scheme:
    //
    // Order of the images corresponds to what blocks on a top-down grid they represent
    // (with player facing north):
    //
    // | 0 |  2 |  1 |
    // | 3 |  5 |  4 |
    // | 6 |  8 |  7 |
    // |   | PP |    |
    //
    // The images also indicate draw order.
    Block,       // 11 images

    // Decoration follows hte same scheme as Block, but also draws the following images
    // (parts of decorations facing "inside" the alley formed in front of the player's view):
    // | 11 |    | 12 |
    // | 13 |    | 14 |
    // | 15 |    | 16 |
    // |  9 | PP | 10 |
    //
    // This suggests that it is possible to draw decorations only in some directions.
    Decoration,  // 17 images

    // Purely determined by distance.  Also used for NPCs.
    Furniture,   // 4 images
}

/// One set of images for one type of wall, decoration, furniture, or NPC
pub struct LabBlock<T> {
    pub images : Vec<LabImage<T>>, // vector of animation frames
    pub num_frames_distant : usize,      // animation frames (the last image may have one frame less)
    pub block_type : LabBlockType,
}

impl LabBlock<IndexedPixmap> {
    pub fn load(resource_nr : usize, data : &[u8]) -> LabBlock<IndexedPixmap> {
	const HDR_TYPE_BLOCK      : u8 = 1;
	const HDR_TYPE_DECORATION : u8 = 2;
	const HDR_TYPE_FURNITURE  : u8 = 3;

	assert!(data[0] == 0);
	let hdr_type = data[1];
	let num_images = data[2] as usize;
	let num_frames = data[3] as usize;

	let num_offsets = if hdr_type == HDR_TYPE_FURNITURE { 36 } else { 34 };
	let xoffsets : Vec<usize> = (0..17).map(|i| decode::u16(&data, 4 + i * 2) as usize).collect();
	let yoffsets : Vec<usize> = (17..34).map(|i| decode::u16(&data, 4 + i * 2) as usize).collect();

	let block_type = match hdr_type {
	    HDR_TYPE_BLOCK      => LabBlockType::Block,
	    HDR_TYPE_DECORATION => LabBlockType::Decoration,
	    HDR_TYPE_FURNITURE  => LabBlockType::Furniture,
	    _                   => { perror!("Unknown block type: {hdr_type:02x}");
				     LabBlockType::Error },
	};

	// HDR_TYPE_FURNITURE has a special feature for in-frame animations that use separate offsets:
	let anim_offsets =
	    if num_offsets == 36 {
		let xoffset = decode::u16(&data, 4 + (34+0) * 2) as usize;
		let yoffset = decode::u16(&data, 4 + (34+1) * 2) as usize;
		if xoffset > 0 || yoffset > 0 { Some((xoffset, yoffset)) } else { None}
	    } else { None };

	let mut image_header_pos = num_offsets * 2 + 4;
	let mut unsorted_pixmaps = vec![];
	let mut unsorted_offsets = vec![];

	debug!("  @ start {resource_nr} = {resource_nr:#x}, {num_images}x{num_frames}");
	for frame in 0..num_frames {
	    let mut pixmap_batch = vec![];
	    let mut offset_batch = vec![];

	    for image_nr in 0..num_images {
		assert!(image_header_pos < data.len() - 1);
		let img_size = decode::u32(&data, image_header_pos) as usize;
		if img_size + image_header_pos + 4 > data.len() {
		    perror!("Inappropriate image size: at LABBLOCK.AMB.{resource_nr:04} index {image_header_pos:x}: img size {img_size:x} vs. max {:x}", data.len());
		}
		let img_start = image_header_pos+4;
		let pixmap = pixmap::new_icon_frame(&data[img_start..img_start+img_size]);
		debug!("   @ decoded {} x {}", pixmap.width, pixmap.height);

		pixmap_batch.push(pixmap);

		let offsets_default = (xoffsets[image_nr], yoffsets[image_nr]);

		let offset_pair = match (image_nr, frame, anim_offsets) {
		    (_, _, None)                => offsets_default,
		    (3, 0, Some(_))             => offsets_default,
		    (3, _, Some(offsets_anim))  => offsets_anim,
		    (_, _, Some(_))             => offsets_default,
		};

		offset_batch.push(offset_pair);

		image_header_pos += 4 + img_size;
	    }
	    unsorted_pixmaps.push(pixmap_batch);
	    unsorted_offsets.push(offset_batch);
	}

	let mut images = vec![];

	for image_nr in 0..num_images {
	    let mut base_pixmap = None;
	    let mut frames = vec![];

	    for frame in 0..num_frames {

		let pixmap = unsorted_pixmaps[frame].pop().unwrap();
		let (xoffset, yoffset) = unsorted_offsets[frame].pop().unwrap();

		let lab_pixmap = LabPixmap {
		    xoffset,
		    yoffset,
		    pixmap,
		};

		match (image_nr, frame, anim_offsets) {
		    (0, 0, Some(_))  => { base_pixmap = Some(lab_pixmap) },
		    _                => frames.push(lab_pixmap),
		};
	    }
	    pdebug!("  Pushed image with {} frames", frames.len());
	    images.push(LabImage {
		base_pixmap,
		pixmaps : frames,
	    });
	}


	return LabBlock {
	    images,
	    num_frames_distant : num_frames,
	    block_type,
	};
    }

    pub fn with_palette(&self, palette : &Palette) -> LabBlock<Pixmap> {
	self.map(&|i : &IndexedPixmap| i.with_palette(palette))
    }

    /// merge base_images with their inferior pixmaps
    pub fn flatten(&self) -> LabBlock<IndexedPixmap> {
	pdebug!("  images: {}", self.images.len());
	LabBlock {
	    images : self.images.iter().map(|i| i.flatten()).collect(),
	    num_frames_distant : self.num_frames_distant,
	    block_type : self.block_type.clone(),
	}
    }
}

impl<T> LabBlock<T> {
    pub fn map<U, F : Fn(&T) -> U>(&self, f : &F) -> LabBlock<U> {
	LabBlock {
	    images : self.images.iter().map(|i| i.map(f)).collect(),
	    num_frames_distant : self.num_frames_distant,
	    block_type : self.block_type.clone(),
	}
    }

    fn image_index_facing(distance : usize, x : isize) -> Option<usize> {
	if x < -1 || x > 1 {
	    return None;
	}
	let xp1 = (x + 1) as usize;
	match distance {
	    3 => { return Some([0, 2, 1][xp1]); }
	    2 => { return Some([3, 5, 4][xp1]); }
	    1 => { return Some([6, 8, 7][xp1]); }
	    _ => { return None; }
	}
    }

    fn image_index_orthogonal(distance : usize, x : isize) -> Option<usize> {
	if x < -1 || x > 1 {
	    return None;
	}
	let xp1 = (x + 1) as usize;
	match distance {
	    3 => { return [Some(11), None, Some(12)][xp1]; }
	    2 => { return [Some(13), None, Some(14)][xp1]; }
	    1 => { return [Some(15), None, Some(16)][xp1]; }
	    0 => { return [Some(9),  None, Some(10)][xp1]; }
	    _ => { return None; }
	}
    }

    /// Find suitable images for this block for a particular relative position to the player's view port
    /// distance: 0..3
    /// x : -1, 0, or 1
    /// Returns: (image facing view port, image orthogonal to view port)
    /// For exmaple, when facing down a corridor, a door on the right would be "orthogonal", while
    /// a door right in front of the player would be "facing" the view port.
    /// Some LabBlocks use the same image for "facing' and "orthogonal", in which case the image is
    /// only reported once.
    /// For distance = 0, we report no "facing" views.
    pub fn image_for(&self, distance: usize, x : isize) -> (Option<&LabImage<T>>, Option<&LabImage<T>>) {
	match self.block_type {
	    LabBlockType::Error => (None, None),

	    LabBlockType::Block |
	    LabBlockType::Decoration => {
		let facing_img = if let Some(facing_index) = LabBlock::<T>::image_index_facing(distance, x) {
		    if facing_index < self.images.len() {
			Some(&self.images[facing_index])
		    } else { None }
		} else { None };
		let orthogonal_img = if let Some(facing_index) = LabBlock::<T>::image_index_orthogonal(distance, x) {
		    if facing_index < self.images.len() {
			Some(&self.images[facing_index])
		    } else { None }
		} else { None };
		(facing_img, orthogonal_img)
	    },
	    LabBlockType::Furniture => {
		(if x == 0 && distance < 4 {
		    Some(&self.images[3 - distance])
		} else {
		    None
		}, None)
	    }
	}
    }
}


impl LabBlock<Pixmap> {
    pub fn as_textures<'a, T>(&self, tc: &'a TextureCreator<T>) -> LabBlock<Texture<'a>> {
	self.map(&|i| i.as_texture(tc))
    }
}

pub struct LabData {
    pub magic_byte : u8,
    pub labblocks :  Vec<usize>,
    pub magic_7 : [u8;7],
}

pub struct LabInfo {
    pub labblocks : Vec<LabBlock<IndexedPixmap>>,
    pub labdata : Vec<LabData>,
}

impl LabData {
    fn load(data : &[u8]) -> LabData {
	assert!(data[0] == 0);

	let num_labblock_refs = data[2] as usize;
	let labblock_slice = &data[3..3 + num_labblock_refs];
	let magic_7_slice = &data[3+num_labblock_refs..];

	assert!(magic_7_slice.len() == 7);

	return LabData {
	    magic_byte : data[1],
	    labblocks : labblock_slice.iter().map(|i| *i as usize - 1).collect(),
	    magic_7 : magic_7_slice.try_into().unwrap(),
	}
    }
}

impl LabInfo {
    pub fn load(labblock_f : &mut DataFile, lab_data_f : &mut DataFile) -> LabInfo {
	let labblocks : Vec<LabBlock<IndexedPixmap>> = (0..labblock_f.num_entries).map(|i| LabBlock::load(i as usize, &labblock_f.decode(i))).collect();
	let labdata : Vec<LabData> = (0..lab_data_f.num_entries).map(|i| LabData::load(&lab_data_f.decode(i))).collect();
	return LabInfo {
	    labblocks,
	    labdata,
	};
    }
}

