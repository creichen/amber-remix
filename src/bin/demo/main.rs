// Copyright (C) 2022-24 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use cli::Command;

use png_codec::Rgba;

use std::path::PathBuf;
use std::{io, fs};


use amber_remix::datafiles::{self, ResourcePath};

use clap::Parser;
mod font;
mod cli;
mod map_demo;
mod gfx_demo;
mod song_player;

fn print_strings(data : &datafiles::AmberstarFiles) {

    let mut map_index = 0;
    for mt in &data.map_text {
	let mut str_index = 0;
	for s in &mt.strings {
	    println!("map[{map_index}].str[{str_index}] = '{s}'");
	    str_index += 1;
	}
	map_index += 1;
    }

    let mut code_index = 0;
    for mt in &data.code_text {
	let mut str_index = 0;
	for s in &mt.strings {
	    println!("code[{code_index}].str[{str_index}] = '{s}'");
	    str_index += 1;
	}
	code_index += 1;
    }
}

// ================================================================================
fn main() -> io::Result<()> {
    env_logger::init();
    let cli = cli::Cli::parse();
    let source = &cli.data.into_os_string().into_string().unwrap();
    let command = match cli.command {
	None    => cli::Command::MapViewer,
	Some(c) => c,
    };

    // Commands that don't use data diretly:
    let completed = match command.clone() {
	Command::Extract{ filename }  => {
	    let mut df = datafiles::DataFile::load(&filename);
	    let dest = cli.output.clone();

	    println!("File type: {}", df.filetype);
	    for i in 0..df.num_entries {
		print!("Extracting {i}/{} \t", df.num_entries);
		let data = df.decode(i);
		let out_filename = format!("{}.{:04}", filename.file_name().unwrap().to_str().unwrap(), i);
		let out_path = dest.join(out_filename);
		println!("  -> writing {} bytes to {}", data.len(), out_path.clone().into_os_string().into_string().unwrap());
		fs::write(out_path, data).expect("Unable to write file");
	    }
	    true
	},
	_ => false,
    };

    if !completed {
	let data = datafiles::AmberstarFiles::new(source);

	match command {
	    Command::Words =>
		for w in 0..data.amberdev.string_fragments.len() {
		    println!("{:4} 0x{:04x}: {}", w, w, data.amberdev.string_fragments.get(w as u16));
		},
	    Command::Strings => print_strings(&data),
	    Command::Song{song:song_nr} =>
		song_player::play_song(&data, song_nr.unwrap_or(0)).unwrap(),
	    Command::PrintSong{song:song_nr} =>
		song_player::print_iter_song(&data, song_nr.unwrap_or(0)),
	    Command::GfxDemo => gfx_demo::show_images(&data),
	    Command::MapViewer => map_demo::show_maps(&data),
	    Command::ListPalettes => {
		let palettes = data.palettes();
		let mut keys: Vec<ResourcePath> = palettes.keys().into_iter().map(|k| k.clone()).collect();
		keys.sort();
		let pad = keys.iter().map(|k| format!("{k}").len()).max().unwrap();
		for key in keys {
		    println!("  {key:0pad$}");
		}
	    }
	    Command::Palette{palette} => {
		let palettes = data.palettes();
		if let Some(palette) = palettes.get(&ResourcePath::from(&palette)) {
		    for (i, col) in palette.colors.iter().enumerate() {
			println!("  {i:02x}: {:x} {:x} {:x} {:x}", col.r >> 4, col.g >> 4, col.b >> 4, col.a >> 4);
		    }
		} else {
		    panic!("Unknown palette '{palette}'");
		}
	    }
	    Command::ListPixmaps => {
		let pixmaps = data.pixmaps();
		let mut keys: Vec<ResourcePath> = pixmaps.keys().into_iter().map(|k| k.clone()).collect();
		keys.sort();
		let pad = keys.iter().map(|k| format!("{k}").len()).max().unwrap();
		for key in keys {
		    if let Some((ref default_palette, pixmap)) = pixmaps.get(&key) {
			let no_default_palette = default_palette.is_empty();
			let pal_str = format!("  \tpalette: {default_palette}");
			let mut s = format!("{key}");
			while s.len() < pad {
			    s += " ";
			}
			println!("  {s}  {}x{}{}", pixmap.width, pixmap.height,
				 if no_default_palette { "" } else { &pal_str });
		    }
		}
	    }
	    Command::ExtractPixmap { pixmap, palette } => {
		let mut dest_file = cli.output.clone();
		let dest_file_str: &str = dest_file.to_str().unwrap();
		if dest_file_str == "" || dest_file_str == "." {
		    dest_file = PathBuf::from(pixmap.clone() + ".png");
		}
		let pixmap_name = pixmap.clone();
		let palettes = data.palettes();
		let pixmaps = data.pixmaps();
		if let Some((default_palette, pixmap)) = pixmaps.get(&ResourcePath::from(&pixmap)) {
		    let palette = match palette {
			None           => default_palette.clone(),
			Some(palname)  => ResourcePath::from(&palname),
		    };
		    if palette.is_empty() {
			panic!("No default palette for pixmap {pixmap_name}, specify palette explicitly");
		    }
		    if let Some(palette) = palettes.get(&palette) {
			let palette: Vec<Rgba> = palette.colors.iter().map(|c| Rgba::new(c.r, c.g, c.b, c.a)).collect();
			let png = png_codec::IndexedImage {
			    height: pixmap.height as u32,
			    width: pixmap.width as u32,
			    pixels: &pixmap.pixels,
			    palette: &palette,
			};
			let encoded = png.encode(5).unwrap();
			std::fs::write(dest_file, &encoded).expect("Failed to save image");
		    } else {
			panic!("Not found: palette {palette}");
		    }
		} else {
		    panic!("Not found: pixmap {pixmap}");
		}

		// let mut keys: Vec<ResourcePath> = palettes.keys().into_iter().map(|&k| k.clone()).collect();
		// keys.sort();
		// let pad = keys.iter().map(|k| format!("{k}").len()).max().unwrap();
		// for key in keys {
		//     println!("{key:pad$}");
		// }
	    }
	    Command::Extract{..}  => {}, // already handled above
	}
    }

    Ok(())
}
