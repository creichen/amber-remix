// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use crate::datafiles::{decode, pixmap};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use super::{DataFile, pixmap::IndexedPixmap};

fn load_background(data : &[u8]) -> Vec<IndexedPixmap> {
    let mut images = vec![];

    let expected_len = decode::u32(data, 0) as usize;
    let num_images = data[4] as usize;
    assert!(expected_len == data.len() - 1 - 4 - 4 * num_images);
    let mut pos = 6;
    debug!("  {num_images} images");
    for _image_nr in 0..num_images {
	let size = decode::u32(data, pos) as usize;
	pos += 4;
	let end = pos + size;
	assert!(end <= data.len());
	let image_data = &data[pos..end];
	debug!("  image from {pos:x}..{end:x} at expected size {size}={size:x} -> {:?}",
	       pixmap::icon_header(image_data));
	let image = pixmap::new_icon_frame(image_data);
	images.push(image);
	pos = end;
    }
    assert!(pos == data.len());

    return images;
}

pub fn load_backgrounds(bgimage_f : &mut DataFile) -> Vec<Vec<IndexedPixmap>> {
    let mut results = vec![];
    for entry_nr in 0..bgimage_f.num_entries {
	debug!("Decoding BACKGRND.AMB.{entry_nr}");
	let bgs = load_background(&bgimage_f.decode(entry_nr));
	// for bg in &bgs {
	//     bg.print();
	// }
	results.push(bgs);
    }
    return results;
}
