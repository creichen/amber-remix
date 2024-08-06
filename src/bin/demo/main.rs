// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use cli::Command;
#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use sdl2::video::Window;
use sdl2::ttf::Sdl2TtfContext;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::{time::Duration, io, fs};

use amber_remix::audio::{Mixer, AQOp, SampleRange, AudioIterator};
use amber_remix::audio::amber::SongIterator;
use amber_remix::datafiles::{music::{BasicSample, Song}, palette::{Palette, self}, pixmap::IndexedPixmap};
use sdl2::{pixels::Color, event::Event, keyboard::{Keycode, Mod}, rect::Rect, render::{Canvas, TextureCreator, Texture, BlendMode, TextureQuery}};

use amber_remix::audio::experiments::{SongPlayerAudioSource, SongTracer};
use amber_remix::{audio::amber, datafiles::pixmap};

use amber_remix::datafiles;
use amber_remix::audio;

use clap::Parser;
mod cli;
mod map_demo;


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

const FONT_SIZE : usize = 12;

struct Font<'a> {
    font : sdl2::ttf::Font<'a, 'a>,
    pub size : usize,
}

impl<'a> Font<'a> {
    pub fn new_ttf(ttf_context : &'a Sdl2TtfContext, path : &str, size : usize) -> Font<'a> {
	// TODO: include font or use the existing one
	let mut font = ttf_context.load_font(path, size as u16).unwrap();
	font.set_style(sdl2::ttf::FontStyle::NORMAL);
	Font {
	    font,
	    size,
	}
    }

    pub fn draw_to(&self, canvas : &mut Canvas<Window>, text : &str, x : isize, y : isize, color : Color) -> (usize, usize) {
	let creator = canvas.texture_creator();
	let surface = self.font
	    .render(text)
	    .blended(color)
	    .map_err(|e| e.to_string()).unwrap();
	let texture = creator
	    .create_texture_from_surface(&surface)
	    .map_err(|e| e.to_string()).unwrap();

	let TextureQuery { width, height, .. } = texture.query();
	let target = Rect::new(x as i32, y as i32, width, height);
	canvas.copy(&texture, None, Some(target)).unwrap();
	(width as usize, height as usize)
    }

    pub fn draw_to_with_outline(&self, canvas : &mut Canvas<Window>, text : &str, x : isize, y : isize, color : Color, outline_color : Color) -> (usize, usize) {
	let creator = canvas.texture_creator();

	let outline_surface = self.font
	    .render(text)
	    .blended(outline_color)
	    .map_err(|e| e.to_string()).unwrap();
	let outline_texture = creator
	    .create_texture_from_surface(&outline_surface)
	    .map_err(|e| e.to_string()).unwrap();

	let TextureQuery { width, height, .. } = outline_texture.query();
	for xdelta in [-1, 1] {
	    for ydelta in [-1, 1] {
		let target = Rect::new(xdelta + x as i32, ydelta + y as i32, width, height);
		canvas.copy(&outline_texture, None, Some(target)).unwrap();
	    }
	}

	let surface = self.font
	    .render(text)
	    .blended(color)
	    .map_err(|e| e.to_string()).unwrap();
	let texture = creator
	    .create_texture_from_surface(&surface)
	    .map_err(|e| e.to_string()).unwrap();


	let TextureQuery { width, height, .. } = texture.query();
	let target = Rect::new(x as i32, y as i32, width, height);
	canvas.copy(&texture, None, Some(target)).unwrap();
	(width as usize, height as usize)
    }
}

// ----------------------------------------
// Audio

enum ISelect {
    Sample,
    Instrument,
    Timbre,
    Monopattern,
}

struct InstrSelect<'a> {
    data : &'a datafiles::AmberstarFiles,
    mixer : &'a mut Mixer,
    song_nr   : usize,
    sample_nr : usize,
    instrument_nr : usize,
    monopattern_nr : usize,
    timbre_nr : usize,
    mode : ISelect,
}

impl<'a> InstrSelect<'a> {
    fn _move_sample(&mut self, dir : isize) {
	self.sample_nr = (((self.sample_nr + self.num_samples()) as isize + dir) as usize) % self.num_samples();
    }
    fn move_sample(&mut self, dir : isize) {
	self._move_sample(dir);
	self.mode = ISelect::Sample;
	self.print_config();
    }
    fn _move_instrument(&mut self, dir : isize) {
	self.instrument_nr = (((self.instrument_nr + self.num_instruments()) as isize + dir) as usize) % self.num_instruments();
    }
    fn move_instrument(&mut self, dir : isize) {
	self._move_instrument(dir);
	self.mode = ISelect::Instrument;
	self.print_config();
    }
    fn _move_timbre(&mut self, dir : isize) {
	self.timbre_nr = (((self.timbre_nr + self.num_timbres()) as isize + dir) as usize) % self.num_timbres();
    }
    fn move_timbre(&mut self, dir : isize) {
	self._move_timbre(dir);
	self.mode = ISelect::Timbre;
	self.print_config();
    }
    fn _move_monopattern(&mut self, dir : isize) {
	self.monopattern_nr = (((self.monopattern_nr + self.num_monopatterns()) as isize + dir) as usize) % self.num_monopatterns();
    }
    fn move_monopattern(&mut self, dir : isize) {
	self._move_monopattern(dir);
	self.mode = ISelect::Monopattern;
	self.print_config();
    }


    fn basicsample(&self) -> BasicSample {
	return self.song().basic_samples[self.sample_nr];
    }
    fn move_song(&mut self, dir : isize) {
	self.song_nr = (((self.song_nr + self.num_songs()) as isize + dir) as usize) % self.num_songs();
	self._move_sample(0);
	self._move_instrument(0);
	self._move_timbre(0);
	self.print_config();
    }

    fn song(&self) -> &'a Song { &self.data.songs[self.song_nr] }
    fn num_monopatterns(&self) -> usize { self.song().monopatterns.len() }
    fn num_timbres(&self) -> usize { self.song().timbres.len() }
    fn num_instruments(&self) -> usize { self.song().instruments.len() }
    fn num_samples(&self) -> usize { self.song().basic_samples.len() }
    fn num_songs(&self)   -> usize { self.data.songs.len() }

    fn play_sample(&mut self, note : usize) {
	let sampleinfo = self.basicsample();
	let sample = AQOp::from(sampleinfo);
	let period = amber::PERIODS[note];
	let freq = amber::period_to_freq(period);
	println!(" .. playing {sampleinfo} at freq {freq}");
	self.mixer.set_iterator(audio::make_note(freq, sample, 10000));
    }

    fn play_instrument(&mut self, note : usize) {
	let ins = &self.song().instruments[self.instrument_nr];
	println!(" .. playing instrument: {}", ins);
	self.mixer.set_iterator(amber::play_instrument(ins, note));
    }

    fn play_timbre(&mut self, note : usize) {
	let ins = &self.song().instruments[self.instrument_nr];
	let timbre = &self.song().timbres[self.timbre_nr];
	println!(" .. playing ====> timbre: {}\n   with default instrument: {}", timbre, ins);
	self.mixer.set_iterator(amber::play_timbre(self.song(), ins, timbre, note));
    }

    fn play_monopattern(&mut self, note : usize) {
	let monopattern = &self.song().monopatterns[self.monopattern_nr];
	println!(" .. playing ====> monopattern(basenote={note}): {}\n", monopattern);
	self.mixer.set_iterator(amber::play_monopattern(self.song(), monopattern, note));
    }

    fn play_song(&mut self) {
	println!("Song:\n{}\n", self.song());
	self.mixer.set_polyiterator(amber::play_song(self.song()));
    }

    fn play(&mut self, note : usize) {
	match self.mode {
	    ISelect::Sample => self.play_sample(note),
	    ISelect::Instrument => self.play_instrument(note),
	    ISelect::Timbre => self.play_timbre(note),
	    ISelect::Monopattern => self.play_monopattern(note),
	}
    }

    fn print_config(&self) {
	match self.mode {
	    ISelect::Sample =>
		println!("Switched to: Song {}/{}, sample {}/{}", self.song_nr, self.num_songs(), self.sample_nr, self.num_samples()),
	    ISelect::Instrument =>
		println!("Switched to: Song {}/{}, instrument {}/{}", self.song_nr, self.num_songs(), self.instrument_nr, self.num_instruments()),
	    ISelect::Timbre =>
		println!("Switched to: Song {}/{}, Timbre {}/{}", self.song_nr, self.num_songs(), self.timbre_nr, self.num_timbres()),
	    ISelect::Monopattern =>
		println!("Switched to: Song {}/{}, Monoapttern {}/{}", self.song_nr, self.num_songs(), self.monopattern_nr, self.num_monopatterns()),
	}
    }
}

// ----------------------------------------
// GfxExplore

struct GfxExplorer<'a> {
    data : &'a datafiles::AmberstarFiles,
    filename : String,
    offset : usize,
    width : usize,
    height : usize,
    palette : usize,
    bitplanes : usize,
    file_index : usize,
    pad : usize,
    transparency : bool,
    print_gfxinfo : bool,
    palettemode: usize,
}

impl<'a> GfxExplorer<'a> {
    fn new(data : &'a datafiles::AmberstarFiles) -> GfxExplorer {
	return GfxExplorer {
	    data,
	    //filename : "COM_BACK.AMB".to_string(),
	    //filename : "BACKGRND.AMB".to_string(),
	    //filename : "MON_GFX.AMB".to_string(),
	    // filename : "CHARDATA.AMB".to_string(),
	    //offset : 0,
	    //pad : 0,

	    // DOCUMENT ME
	    // // --------------------------------------------------------------------------------
	    // filename : "AMBERDEV.UDO".to_string(),
	    // //offset: 0x33d70,
	    // offset:   0x28024,
	    // pad : 0,
	    // width : 16, // try 16, 64 and 128
	    // height : 16,
	    // palette : 0,
	    // bitplanes : 4, // usually 4
	    // file_index : 0,

	    // DOCUMENT ME
	    // // --------------------------------------------------------------------------------
	    // filename : "CHARDATA.AMB".to_string(),
	    // //offset: 0x33d70,
	    // offset:   0x6b0,
	    // pad : 0,
	    // width : 32, // try 16, 64 and 128
	    // height : 34,
	    // palette : 0,
	    // bitplanes : 4, // usually 4
	    // file_index : 9,

	    // // --------------------------------------------------------------------------------
	    // filename : "PUZZLE.ICN".to_string(),
	    // //offset: 0x33d70,
	    // offset:   0x0,
	    // pad : 0,
	    // width : 16, // try 16, 64 and 128
	    // height : 16,
	    // palette : 0,
	    // bitplanes : 4, // usually 4
	    // file_index : 0,

	    // // --------------------------------------------------------------------------------
	    // filename : "COM_BACK.AMB".to_string(),
	    // offset:   0x0,
	    // pad : 0,
	    // width : 176, // try 16, 64 and 128
	    // height : 112,
	    // palette : 0,
	    // bitplanes : 4, // usually 4
	    // file_index : 0,

	    // // // --------------------------------------------------------------------------------
	    // filename : "TACTIC.ICN".to_string(),
	    // //offset: 0x33d70,
	    // offset:   0x0,
	    // pad : 0,
	    // width : 16, // try 16, 64 and 128
	    // height : 16,
	    // palette : 0,
	    // bitplanes : 4, // usually 4
	    // file_index : 0,

	    // --------------------------------------------------------------------------------
	    filename : "F_T_ANIM.ICN".to_string(),
	    //offset: 0x33d70,
	    offset:   0x0,
	    pad : 0,
	    width : 16, // try 16, 64 and 128
	    height : 16,
	    palette : 0,
	    bitplanes : 4, // usually 4
	    file_index : 0,

	    transparency : false,
	    print_gfxinfo : true,
	    palettemode: 0,
	};
    }

    pub fn mod_offset(&mut self, delta : isize) { self.offset = isize::max(0, self.offset as isize + delta) as usize; self.info(); }
    pub fn mod_width(&mut self, delta : isize) { self.width = isize::max(0, self.width as isize + delta) as usize;  self.info(); }
    pub fn mod_height(&mut self, delta : isize) { self.height = isize::max(0, self.height as isize + delta) as usize;  self.info(); }
    pub fn mod_pad(&mut self, delta : isize) { self.pad = isize::max(0, self.pad as isize + delta) as usize;  self.info(); }
    pub fn mod_palette(&mut self, delta : isize) { self.palette = isize::min((self.data.amberdev_palettes.len() + 2) as isize, isize::max(0, self.palette as isize + delta)) as usize;  self.info(); }
    pub fn mod_bitplanes(&mut self, delta : isize) { self.bitplanes = isize::min(5, isize::max(2, self.bitplanes as isize + delta)) as usize;  self.info(); }
    pub fn mod_palettemode(&mut self, delta : isize) { self.palettemode = isize::max(0, self.palettemode as isize + delta) as usize;  self.info(); }
    pub fn mod_file_index(&mut self, delta : isize) { self.file_index = isize::max(0, self.file_index as isize + delta) as usize;  self.info(); }
    pub fn toggle_transparency(&mut self) { self.transparency = !self.transparency; self.info(); println!("transparency = {}", self.transparency); }

    fn print_config(&self) {
	println!("[GFX] {} off:{} padding:{}, (0x{:x}) size:{}x{}, bp:{}, pal:{}, palettemode:{}",
		 self.filename, self.offset, self.pad, self.offset, self.width, self.height, self.bitplanes, self.palette,
		 self.palettemode);
    }

    #[allow(unused)]
    fn get_palette(&self) -> Palette {
	//if self.palettemode == 0 {
	    let palettes = &self.data.amberdev_palettes;

	    if self.palette == palettes.len() {
		return palette::TEST_PALETTE.clone();
	    } else if self.palette > palettes.len() {
		return self.data.tiles[self.palette - palettes.len() - 1].palette.clone();
	    }
	let mut xpal = palettes[self.palette].clone();
	if self.palettemode > 0 {
	    xpal = xpal.replacing(0xc, 3, &self.data.amberdev[0x31ef8 + self.palettemode * 2..]);
	}
	return xpal;
	// } else {
	//     return palette::new(&self.data.amberdev[0x313d8 + 2*self.palettemode..], 16);
	// }
    }

    fn info(&mut self) {
	self.print_gfxinfo = true;
    }

    // For ICN files
    fn _embedded_palette(&mut self) -> Palette {
	let mut xdata = self.data.load(&self.filename);
	self.file_index %= xdata.num_entries as usize;
	let bytes = xdata.decode(self.file_index as u16);
	return palette::new_with_header(&bytes[0x7d2..], 255/7);
    }

    fn pixmaps(&mut self) -> Vec<IndexedPixmap> {
	const PRINT_PADDING : bool = false;

	let mut results = vec![];
	let mut xdata = self.data.load(&self.filename);
	self.file_index %= xdata.num_entries as usize;
	let bytes = xdata.decode(self.file_index as u16);

	let imgsize = (((self.width + 15) / 16) * 2) * self.height * self.bitplanes;
	let padded_imgsize = imgsize + self.pad;
	let count = (bytes.len() as usize - self.offset) / padded_imgsize;

	if self.print_gfxinfo {
	    self.print_config();
	    println!("[GFX] assuming {imgsize} (0x{imgsize:x}) (padded: {padded_imgsize}, 0x{padded_imgsize:x}) bytes per image -> {count} images, {} (0x{:x}) bytes left over",
		     bytes.len() - self.offset - (padded_imgsize * count),
		     bytes.len() - self.offset - (padded_imgsize * count),
	    );
	    self.print_gfxinfo = false;
	}

	for i in 0..count {
	    let offset = self.offset + padded_imgsize * i;
	    if PRINT_PADDING && self.pad > 0 {
		print!("Padding for img #{i:03x}: ");
		let full_slice = &bytes[offset..offset+padded_imgsize];
		for i in 0..self.pad {
		    print!("{:02x} ", full_slice[i]);
		}
		println!();
	    }
	    let img_slice = &bytes[offset+self.pad..offset+padded_imgsize];
	    let pixmap = pixmap::new(img_slice, self.width, self.height, self.bitplanes);
	    results.push(pixmap);
	}
	return results;
    }

    fn print_img(&mut self, which : usize) {
	let pixmaps = self.pixmaps();
	let mut colors_used = [false;256];
	if which < pixmaps.len() {
	    let pixmap = &pixmaps[which];
	    for y in 0..pixmap.height {
		for x in 0..pixmap.width {
		    let pos = y * pixmap.width + x;
		    let pixel = pixmap.pixels[pos];
		    print!("{:2x}", pixel);
		    colors_used[pixel as usize] = true;
		}
		println!("");
	    }
	}
	print!(" Colours used: ");
	for (i, c) in colors_used.iter().enumerate() {
	    if *c {
		print!("{:x}", i);
	    }
	}
	println!();
    }

    fn make_pixmaps<'b, T>(&mut self, texcreate : &'b TextureCreator<T>) -> Vec<Texture<'b>> {
	let palette = self.get_palette();
//	let palette = self.embedded_palette();
	let mut results = vec![];
	let pixmaps = self.pixmaps();
	for pixmap in pixmaps {
	    let pixmap = if self.transparency {
		let palette2 = palette.with_transparency(0);
		pixmap.with_palette(&palette2)
	    } else {
	     	pixmap.with_palette(&palette)
	    };
	    let mut texture = pixmap.as_texture(texcreate);
	    texture.set_blend_mode(BlendMode::Blend);
	    results.push(texture);
	}

	return results;
    }
}

#[allow(unused)]
fn draw_sampledata<'a>(full_data : &'a [i8], canvas : &mut Canvas<sdl2::video::Window>, ybase : i32, sampledata : SampleRange) {
    let pos = sampledata.start;
    let len = sampledata.len;
    let data = &full_data[pos..pos+len];
    let xfactor : i32 = ((len+2799) / 2800) as i32;

    let startx : i32 = 10;
    let mut x : i32 = 0;
    for y in data {
	canvas.draw_point(sdl2::rect::Point::new(startx + (x / xfactor), ybase + ((*y) as i32 >> 2))).unwrap();
	x += 1;
    }

    canvas.set_draw_color(Color::RGBA(255, 0, 128, 255));
    canvas.draw_line(sdl2::rect::Point::new(startx -3, ybase),
		     sdl2::rect::Point::new(startx -3, ybase - 25)).unwrap();
    canvas.draw_line(sdl2::rect::Point::new(startx + (x / xfactor) +3, ybase),
		     sdl2::rect::Point::new(startx + (x / xfactor) +3, ybase - 25)).unwrap();
}

fn show_images(data : &datafiles::AmberstarFiles) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem.window("amber-remix", 3000, 1600)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();
    let creator = canvas.texture_creator();

    // let mut audiocore = audio::init(&sdl_context);
    // let mut mixer = audiocore.start_mixer(&data.sample_data.data[..]);
    // let mut instr = InstrSelect {
    // 	data, mixer:&mut mixer,
    // 	song_nr : 0,
    // 	sample_nr : 0,
    // 	instrument_nr : 0,
    // 	timbre_nr : 0,
    // 	monopattern_nr : 0,
    // 	mode : ISelect::Instrument };

    let mut gfxexplore = GfxExplorer::new(data);
    let mut focus_img : usize = 0;

    canvas.set_draw_color(Color::RGBA(0, 255, 255, 255));
    canvas.clear();
    canvas.present();
    let mut event_pump = sdl_context.event_pump().unwrap();
    let mut i = 0;
    'running: loop {
        i = (i + 1) & 0x3f;
        canvas.set_draw_color(Color::RGBA(0, 0, 32 + i, 0xff));
	//canvas.set_draw_color(Color::RGB(i, 64, 128 - (i>>1)));
        canvas.clear();

	// for j in 0..8*6 {
	//     let addr = 0x31efa + j * 2;
	//     let d = &data.amberdev[addr..addr+2];
	//     let r = ((d[0] & 0xf) ) << 5;
	//     let g = (((d[1] >> 4) & 0xf) ) << 5;
	//     let b = ((d[1] & 0xf) ) << 5;
	//     canvas.set_draw_color(Color::RGBA(r, g, b, 255));
	//     let height = 8;
	//     canvas.fill_rect(sdl2::rect::Rect::new(1800, 400 + (j as i32)*(height as i32),
	// 					   2200, height)).unwrap();
	// }

	for j in 0..data.pics80.len() {
	    let img = &data.pics80[j];
	    let creator = canvas.texture_creator();
	    let texture = img.as_texture(&creator);
	    canvas.copy(&texture, None, Some(Rect::new(j as i32 * (img.width as i32 + 8), 0, img.width as u32, img.height as u32))).unwrap();
	}

	// for (index, j) in [5, 15, 24, 25].iter().enumerate() {
	//     let img = &data.pics80[*j];
	//     let creator = canvas.texture_creator();
	//     let texture = img.as_texture(&creator);
	//     canvas.copy(&texture, None, Some(Rect::new(index as i32 * (img.width as i32 + 8), 0, img.width as u32, img.height as u32))).unwrap();
	// }

	for (j, img) in data.combat_bg_pictures.iter().enumerate() {
		let mut texture = img.as_texture(&creator);
		texture.set_blend_mode(BlendMode::Blend);
		let TextureQuery { width, height, .. } = texture.query();
		canvas.copy(&texture,
			    Rect::new(0, 0, width, height),
			    Some(Rect::new((j % 6) as i32 * 200 + 100, (j / 6) as i32 * 120 + 1200, img.width as u32, img.height as u32))).unwrap();
	}

	for j in 0..data.monster_gfx.len() {
	    let imgseq = &data.monster_gfx[j];
	    //let pal = gfxexplore.get_palette().with_transparency(0);
	    for (y, mgfx) in imgseq.iter().enumerate() {
		let img = mgfx;//.with_palette(&pal);
		let mut texture = img.as_texture(&creator);
		texture.set_blend_mode(BlendMode::Blend);
		let TextureQuery { width, height, .. } = texture.query();
		canvas.copy(&texture,
			    Rect::new(0, 0, width, height),
			    Some(Rect::new(j as i32 * 60 + 1500, 100 + y as i32 * 60, img.width as u32, img.height as u32))).unwrap();
	    }
	}

	{
	    let mut xpos = 10;
	    let mut ypos = 200;
	    let creator = canvas.texture_creator();
	    let textures = gfxexplore.make_pixmaps(&creator);
	    let src_width = gfxexplore.width;
	    let src_height = gfxexplore.height;
	    let width = src_width * 2;
	    let height = src_height * 2;
	    for t in &textures {
		canvas.copy(&t,
			    Rect::new(0, 0, src_width as u32, src_height as u32),
			    Rect::new(xpos as i32, ypos as i32, width as u32, height as u32)).unwrap();
		xpos += width + 5;
		if xpos + width > 3000 {
		    xpos = 10;
		    ypos += height + 10;
		}
	    }
	    if focus_img < textures.len() {
		canvas.copy(&textures[focus_img],
			    Rect::new(0, 0, src_width as u32, src_height as u32),
			    Rect::new(0, (ypos + height + 10) as i32, (width * 4) as u32, (height * 4) as u32)).unwrap();
	    }
	}

	// let img = &data.pic_intro;
	// let creator = canvas.texture_creator();
	// let texture = &data.pic_intro.as_texture(&creator);
	// canvas.copy(&texture, None, Rect::new(100, 200, img.width, img.height)).unwrap();

	// let sampledata = instr.basicsample();

	// canvas.set_draw_color(Color::RGB(150, 255, 0));
	// draw_sampledata(&data.sample_data.data[..], &mut canvas, 300, sampledata.attack);
	// if let Some(sustain) = sampledata.looping {
	//     canvas.set_draw_color(Color::RGB(1, 255, 0));
	//     draw_sampledata(&data.sample_data.data[..], &mut canvas, 500, sustain);
	// }

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
		    break 'running
                },
                Event::KeyDown { keycode : Some(kc), repeat:false, keymod, .. } => {
		    let mut stride = 1;
		    if !(keymod & Mod::RSHIFTMOD).is_empty() {
			stride = 64;
		    }
		    if !(keymod & Mod::RALTMOD).is_empty() {
			stride <<= 2;
		    }
		    if !(keymod & Mod::LSHIFTMOD).is_empty() {
			stride *= -1;
		    }
		    match kc {
			Keycode::F1           => gfxexplore.mod_offset(stride),
			Keycode::F2           => gfxexplore.mod_pad(stride),
			Keycode::F3           => { focus_img = if -stride > focus_img as isize { 0 } else { (stride + focus_img as isize) as usize }; println!("focus: {focus_img}");},
			Keycode::F4           => gfxexplore.toggle_transparency(),
			Keycode::F5           => gfxexplore.mod_palette(stride),
			Keycode::F7           => gfxexplore.mod_width(stride),
			Keycode::F8           => gfxexplore.mod_height(stride),
			Keycode::F9           => gfxexplore.mod_bitplanes(stride),
			Keycode::F10          => gfxexplore.mod_palettemode(stride),
			Keycode::F11          => gfxexplore.print_img(focus_img),
			Keycode::F12          => gfxexplore.mod_file_index(stride),

			// Keycode::LeftBracket  => instr.move_song(-1),
			// Keycode::RightBracket => instr.move_song(1),
			// Keycode::Minus        => instr.move_sample(-1),
			// Keycode::Equals       => instr.move_sample(1),
			// Keycode::Quote        => instr.move_instrument(-1),
			// Keycode::Backslash    => instr.move_instrument(1),
			// Keycode::Period       => instr.move_timbre(-1),
			// Keycode::Slash        => instr.move_timbre(1),
			// Keycode::Kp7          => instr.move_monopattern(-1),
			// Keycode::Kp9          => instr.move_monopattern(1),

			// Keycode::Return       => instr.play_song(),
			// Keycode::Space        => instr.play(0),
			// Keycode::Z            => instr.play(12),
			// Keycode::S            => instr.play(13),
			// Keycode::X            => instr.play(14),
			// Keycode::D            => instr.play(15),
			// Keycode::C            => instr.play(16),
			// Keycode::V            => instr.play(17),
			// Keycode::G            => instr.play(18),
			// Keycode::B            => instr.play(19),
			// Keycode::H            => instr.play(20),
			// Keycode::N            => instr.play(21),
			// Keycode::J            => instr.play(21),
			// Keycode::M            => instr.play(23),

			// Keycode::Q            => instr.play(24),
			// Keycode::Num2         => instr.play(25),
			// Keycode::W            => instr.play(26),
			// Keycode::Num3         => instr.play(27),
			// Keycode::E            => instr.play(28),
			// Keycode::R            => instr.play(29),
			// Keycode::Num5         => instr.play(30),
			// Keycode::T            => instr.play(31),
			// Keycode::Num6         => instr.play(32),
			// Keycode::Y            => instr.play(33),
			// Keycode::Num7         => instr.play(34),
			// Keycode::U            => instr.play(35),

			// Keycode::I            => instr.play(36),
			// Keycode::Num9         => instr.play(37),
			// Keycode::O            => instr.play(38),
			// Keycode::Num0         => instr.play(39),
			// Keycode::P            => instr.play(40),
			    _ => { println!("<ESC>: quit\n [/] : song\n -|=: sample\n '|\\: instrument\n .|/: timbre\n  Num7/Num9: Monopattern\nzsxdc.../q2w3e... -> play note; Space: play zero note (monopatterns)")},
		    }
                },
                _ => {}
            }
        }
        // The rest of the game loop goes here...

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 50));
    }
    // mixer.shutdown();
}

fn print_iter_song(data : &datafiles::AmberstarFiles, song_nr : usize) {
    let song = &data.songs[song_nr];
    println!("{}", song);
    let mut poly_it = SongIterator::new(&song,
					song.songinfo.first_division,
					song.songinfo.last_division);
    for i in 0..32 {
	let mut d = VecDeque::<AQOp>::new();
	poly_it.channels[0].next(&mut d);
	println!("--- tick {i:02x}\n");
	for dd in d {
	    println!(" {dd:?}\n");
	}
    }
}

const SAMPLE_RATE : usize = audio::experiments::SAMPLE_RATE;

fn float_to_i16(x: f32) -> i16 {
    if x > 1.0 { 0x3fff } else
    { if x < -1.0 { -0x4000 } else { (x * 32767.0) as i16 }}
}

// fn float_buffer_to_i16(input : &[f32]) -> Vec<i16> {
//     let mut result = Vec::new();
//     for xr in input {
// 	// FIXME: why can't I iterate more elegantly?
// 	let x = *xr;
// 	result.push(float_to_i16(x));
//     }
//     result
// }

fn float_buffers_merge_to_i16(input_l : &[f32], input_r: &[f32]) -> Vec<i16> {
    let mut result = Vec::new();
    for xr in 0..input_l.len() {
	result.push(float_to_i16(input_l[xr]));
	result.push(float_to_i16(input_r[xr]));
    }
    result
}

// --------------------------------------------------------------------------------

type InfoFunction = fn(&mut PaginatedWriter, &ArcDemoSongTracer, CurrentSongInfo) -> ();

struct CurrentSongInfo<'a> {
    song: &'a Song,
    tick: usize,
    song_nr: usize,
    info_functions: &'a [(Keycode, &'a str, InfoFunction)],
}

const COLOR_WHITE : Color = Color::RGBA(0xff, 0xff, 0xff, 0xff);
const COLOR_GREEN : Color = Color::RGBA(0, 0xff, 0, 0xff);
const COLOR_CYAN : Color = Color::RGBA(0, 0xff, 0xff, 0xff);
const COLOR_YELLOW : Color = Color::RGBA(0xff, 0xff, 0, 0xff);

const COLOR_CHANNEL_SEP : Color = Color::RGBA(0, 0x80, 0xff, 0xff);
const COLOR_CHANNEL : Color = COLOR_CYAN;

struct PaginatedWriter<'a> {
    base_xpos: isize,
    base_ypos: isize,
    xpos: isize,
    ypos: isize,
    max_height: isize,
    max_xwidth_current: isize,
    max_ywidth_current: isize,
    font: &'a Font<'a>,
    canvas: &'a mut Canvas<Window>,
    pub color: Color,
}

impl<'a> PaginatedWriter<'a> {
    fn new(xpos: isize, ypos: isize, max_height: isize,
	   font: &'a Font<'a>,
	   canvas: &'a mut Canvas<Window>) -> Self {
	PaginatedWriter {
	    base_xpos: xpos,
	    base_ypos: ypos,
	    xpos,
	    ypos,
	    max_height,
	    max_xwidth_current: 0,
	    max_ywidth_current: 0,
	    font,
	    canvas,
	    color: COLOR_WHITE,
	}
    }

    fn font_size(&self) -> isize {
	return self.font.size as isize;
    }

    fn line_height(&self) -> isize {
	return self.font_size() + 1;
    }

    fn column_width(&self) -> isize {
	return self.font_size() * 4;
    }

    fn print(&mut self, s: &str) {
	let (width, height) = self.font.draw_to(&mut self.canvas, s,
						self.xpos as isize, self.ypos as isize,
						self.color);
	self.xpos += width as isize;
	self.max_xwidth_current = isize::max(self.max_xwidth_current, width as isize);
	self.max_ywidth_current = isize::max(self.max_ywidth_current, height as isize);
    }

    fn new_column(&mut self) {
	self.ypos = self.base_ypos;
	self.base_xpos = self.base_xpos + self.max_xwidth_current + self.column_width();
	self.xpos = self.base_xpos;
    }

    fn newline(&mut self) {
	self.ypos += self.line_height();
	if self.ypos - self.base_ypos + self.line_height() < self.max_height {
	    self.xpos = self.base_xpos;
	} else {
	    self.new_column();
	}
    }

    fn println(&mut self, s: &str) {
	self.print(s);
	self.newline();
    }

    fn set_color(&mut self, c: Color) {
	self.color = c;
    }
}

fn songinfo_help(wr: &mut PaginatedWriter, _tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    wr.println("[F11] / [F12]  : Change song");
    wr.println("[ / ]          : Zoom");
    wr.println("KPad   <- ->   : move in song");
    wr.println("KPad End  PgDn : move in song (single step)");
    wr.println("Enter          : Follow song");
    for (kc, description, _) in song_info.info_functions.iter() {
	wr.println(&format!("{:15}: {description}", format!("{kc}")));
    }
}

fn songinfo_divisions(wr: &mut PaginatedWriter, tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    let status = tracer.get_channel_updates("division", song_info.tick);

    wr.set_color(COLOR_YELLOW);
    wr.println(" --- [Divisions] ---");
    for (i, div) in song_info.song.divisions.iter().enumerate() {
	wr.set_color(COLOR_WHITE);
	status.highlight_if_match(i as isize, wr, COLOR_YELLOW);
	wr.print(&format!("D{i:02x} {div}"));
	status.print_if_match(i as isize, wr, COLOR_YELLOW);
	wr.newline();
    }
}

fn songinfo_monopatterns(wr: &mut PaginatedWriter, tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    let status = tracer.get_channel_updates("monopattern", song_info.tick);

    wr.set_color(COLOR_YELLOW);
    wr.println(" --- [Monopatterns] ---");
    for (i, pat) in song_info.song.monopatterns.iter().enumerate() {
	wr.set_color(COLOR_WHITE);
	status.highlight_if_match(i as isize, wr, COLOR_YELLOW);
	wr.print(&format!("P#{i:02x} {pat}"));
	status.print_if_match(i as isize, wr, COLOR_YELLOW);
	wr.newline();
    }
}

fn songinfo_timbres(wr: &mut PaginatedWriter, tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    let status = tracer.get_channel_updates("timbre", song_info.tick);

    wr.set_color(COLOR_YELLOW);
    wr.println(" --- [Timbres] ---");
    wr.set_color(COLOR_WHITE);
    for (i, timbre) in song_info.song.timbres.iter().enumerate() {
	wr.set_color(COLOR_WHITE);
	status.highlight_if_match(i as isize, wr, COLOR_YELLOW);
	wr.print(&format!("T:{i:02x} {timbre}"));
	status.print_if_match(i as isize, wr, COLOR_YELLOW);
	wr.newline();
    }
}

fn songinfo_instruments_samples(wr: &mut PaginatedWriter, tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    let status = tracer.get_channel_updates("instrument", song_info.tick);

    wr.set_color(COLOR_YELLOW);
    wr.println(" --- [Instruments] ---");
    wr.set_color(COLOR_WHITE);
    for (i, instr) in song_info.song.instruments.iter().enumerate() {
	wr.set_color(COLOR_WHITE);
	status.highlight_if_match(i as isize, wr, COLOR_YELLOW);
	wr.print(&format!("I#{i:02x} {instr}"));
	status.print_if_match(i as isize, wr, COLOR_YELLOW);
	wr.newline();
    }
    wr.set_color(COLOR_YELLOW);
    wr.println(" --- [Samples] ---");
    wr.set_color(COLOR_WHITE);
    for (i, s) in song_info.song.basic_samples.iter().enumerate() {
	wr.println(&format!("S:{i:02x} {s}"));
    }
}

fn songinfo_channel_stat(wr: &mut PaginatedWriter, tracer: &ArcDemoSongTracer, song_info: CurrentSongInfo) {
    wr.new_column();
    for i in 0..4 {
	wr.set_color(COLOR_CHANNEL);
	wr.println(&format!(" --- [Channel #{i}] ---"));
	let frame = tracer.frame(i, song_info.tick);
	let mut updates = frame.updates.clone();
	updates.sort_by(|a, b| a.0.cmp(b.0));
	for (n, v) in updates {
	    wr.set_color(COLOR_YELLOW);
	    wr.print(&format!("{n:14}: "));
	    wr.set_color(COLOR_WHITE);
	    wr.println(&format!("{v}"));
	}
	wr.new_column();
    }
}

/// Generic channel status info used by the visualisers
struct ChannelStatusInfo {
    info: [Option<isize>; 4],
}

impl ChannelStatusInfo {
    fn new(info: [Option<isize>; 4]) -> Self {
	ChannelStatusInfo {
	    info,
	}
    }

    fn matches(&self, pos: isize) -> Vec<u8> {
	let mut result = vec![];
	for (i, c) in self.info.iter().enumerate() {
	    if let Some(v) = c {
		if *v == pos {
		    result.push(i as u8);
		}
	    }
	}
	result
    }

    fn highlight_if_match(&self,
			  pos: isize,
			  wr: &mut PaginatedWriter,
			  highlight_color: Color) {
	let matches = self.matches(pos);
	if matches.is_empty() {
	    return;
	}
	wr.set_color(highlight_color);
    }

    fn print_if_match(&self,
		      pos: isize,
		      wr: &mut PaginatedWriter,
		      highlight_color: Color) {
	let matches = self.matches(pos);
	if matches.is_empty() {
	    return;
	}
	wr.set_color(COLOR_CHANNEL_SEP);
	wr.print("[");
	wr.set_color(COLOR_CHANNEL);
	if matches.len() == 4 {
	    wr.print("*");
	} else {
	    for c in matches {
		wr.print(&format!("{c}"));
	    }
	}
	wr.set_color(COLOR_CHANNEL_SEP);
	wr.print("]");
    }
}

#[derive(Clone)]
struct ChannelTraceFrame {
    messages: Vec<Vec<String>>,
    data: Vec<f32>,
    updates: Vec<(&'static str, isize)>,
}

impl ChannelTraceFrame {
    fn new() -> ChannelTraceFrame {
	ChannelTraceFrame {
	    messages: vec![Vec::new(), Vec::new(), Vec::new()],
	    data: Vec::new(),
	    updates: Vec::new(),
	}
    }

    fn category_index(s: &'static str) -> usize {
	match s {
	    "monopattern" => 0,
	    "note" => 1,
	    "speed" => 1,
	    "avolume" => 1,
	    _ => 2,
	}
    }
}

struct TraceFrame {
    channels: [ChannelTraceFrame; 4],
}

impl TraceFrame {
    fn new() -> TraceFrame {
	TraceFrame {
	    channels: [
		ChannelTraceFrame::new(),
		ChannelTraceFrame::new(),
		ChannelTraceFrame::new(),
		ChannelTraceFrame::new(),
	    ]
	}
    }
}

struct DemoSongTracer {
    info: Vec<TraceFrame>,
    latest_tick: usize,
}

impl DemoSongTracer {
    fn new() -> DemoSongTracer {
	DemoSongTracer {
	    info: Vec::new(),
	    latest_tick: 0,
	}
    }

    fn ensure_info(&mut self, tick: usize) {
	while self.info.len() < tick + 1 {
	    self.info.push(TraceFrame::new());
	}
    }
}

impl SongTracer for DemoSongTracer {
    fn trace_buf(&mut self, tick: usize, channel: u8, buf: Vec<f32>) {
	self.ensure_info(tick);
	self.info[tick].channels[channel as usize].data = buf;
	self.latest_tick = usize::max(tick, self.latest_tick);
    }

    fn change_song(&mut self) {
	self.info = Vec::new();
	self.latest_tick = 0;
    }

    fn trace_message(&mut self, tick: usize, channel: u8,
		     _subsystem: &'static str,
		     category: &'static str,
		     message: String) {
	self.ensure_info(tick);
	self.info[tick].channels[channel as usize].messages[ChannelTraceFrame::category_index(category)].push(
	    format!("{category}: {message}"));
    }
    fn trace_message_num(&mut self, tick: usize, channel: u8,
			 _subsystem: &'static str,
			 category: &'static str,
			 message: isize) {
	self.ensure_info(tick);
	self.info[tick].channels[channel as usize].updates.push((category.into(), message));
	//println!("============= NUM {channel}:{tick}:{category}:{message} ==========");
    }
}

struct ArcDemoSongTracer {
    t : Arc<Mutex<DemoSongTracer>>,
}

impl ArcDemoSongTracer {
    fn new() -> ArcDemoSongTracer {
	ArcDemoSongTracer {
	    t: Arc::new(Mutex::new(DemoSongTracer::new()))
	}
    }

    fn tracer(&self) -> Arc<Mutex<DemoSongTracer>> {
	return self.t.clone();
    }

    // Get most recent numeric logger info
    fn get_latest_update_for_channel(&self, channel: u8, category: &'static str, from_tick: usize) -> Option<isize> {
	for i in (0..=from_tick).rev() {
	    let frame = self.frame(channel, i);
	    for (n, value) in frame.updates {
		if n == category {
		    return Some(value);
		}
	    }
	}
	None
    }

    fn get_channel_updates(&self, category: &'static str, from_tick: usize) -> ChannelStatusInfo {
	return ChannelStatusInfo::new([
	    self.get_latest_update_for_channel(0, category, from_tick),
	    self.get_latest_update_for_channel(1, category, from_tick),
	    self.get_latest_update_for_channel(2, category, from_tick),
	    self.get_latest_update_for_channel(3, category, from_tick),
	]);
    }

    fn frame(&self, channel: u8, tick: usize) -> ChannelTraceFrame {
	let guard = self.t.lock().unwrap();
	if guard.info.len() <= tick {
	    return ChannelTraceFrame::new();
	}
	return guard.info[tick].channels[channel as usize].clone();
    }

    fn latest_tick(&self) -> usize {
	let guard = self.t.lock().unwrap();
	return guard.latest_tick;
    }

    fn tick_length(&self) -> usize {
	let t = self.frame(0, 0);
	return t.data.len();
    }


    fn draw_info<F>(&self,
		    canvas: &mut Canvas<Window>,
		    pos: Rect,
		    font: &Font,
		    current_song_info: CurrentSongInfo,
		    song_info_fn: F)
	where F: Fn(&mut PaginatedWriter, &ArcDemoSongTracer, CurrentSongInfo) -> () {

	let mut pw = PaginatedWriter::new(pos.x as isize, pos.y as isize,
					  pos.h as isize,
					  font,
					  &mut* canvas);
	pw.set_color(COLOR_GREEN);
	pw.print(&format!("Song {:02x}", current_song_info.song_nr));
	pw.set_color(COLOR_YELLOW);
	pw.println("  [F1] for help");
	pw.set_color(COLOR_WHITE);
	song_info_fn(&mut pw, &self, current_song_info);
    }

    fn draw_channel_info_at(&self,
			    data: &ChannelTraceFrame,
			    canvas: &mut Canvas<Window>,
			    tick: usize,
			    pos: Rect,
			    font: &Font) {
	let mut yoffset = 0;
	let s = format!("{tick}");
	font.draw_to(canvas, &s,
		     pos.x as isize, (pos.y + yoffset) as isize,
		     Color::RGBA(0x00, 0xff, 0x20, 0xff));
	yoffset += 13;

	for (_i, vecs) in data.messages.iter().enumerate() {
	    for s in vecs {
		font.draw_to(canvas, &s,
			     pos.x as isize, (pos.y + yoffset) as isize,
			     Color::RGBA(0xff, 0xff, 0x20, 0xff));

		yoffset += 13;
	    }
	}
    }

    fn draw_audio_track(&self, canvas: &mut Canvas<Window>,
			pos: Rect,
			channel: u8,
			start_tick: usize,
			downscale: i32,
			font: &Font,
    ) {
	let tick_length = self.tick_length();
	let height = pos.h;
	let y_baseline = pos.y + height / 2;
	canvas.set_draw_color(Color::RGBA(0, 0, 0, 255));
	canvas.fill_rect(pos).unwrap();
	canvas.set_draw_color(Color::RGBA(0, 0, 128, 255));
	canvas.draw_line(sdl2::rect::Point::new(pos.x, y_baseline),
			 sdl2::rect::Point::new(pos.x + pos.w - 1, y_baseline)).unwrap();
	let mut last_y = 0.0;
	let mut xpos = 0;

	let mut tick = start_tick + 1; // fake value to force update
	let mut next_tick = start_tick;
	let mut data = ChannelTraceFrame::new();

	let mut sample_within_tick = 0;
	let xfrac = 1.0 / downscale as f32;
	let y_baseline = y_baseline as f32;
	let mut last_ypos = y_baseline;
	let xoffset = pos.x as f32;
	while xpos < pos.w * downscale {
	    let last_xpos = if xpos == 0 { 0 } else { xpos - 1 };
	    if next_tick != tick {
		tick = next_tick;
		data = self.frame(channel, tick);
		self.draw_channel_info_at(&data, canvas,
					  tick,
					  sdl2::rect::Rect::new(xoffset as i32 + (xpos / downscale), pos.y + pos.h,
								// no idea what to putthere yet
								100, 100),
					  font);
	    }
	    let y = if sample_within_tick >= data.data.len() { 0.0 } else {data.data[sample_within_tick]};
	    let ypos = y_baseline + ((y * height as f32) * 0.49);
	    if y == last_y {
		// single pixel
		canvas.set_draw_color(Color::RGBA(0xa0, 0xa0, 0xa0, 0xff));
		canvas.draw_fpoint(sdl2::rect::FPoint::new(xoffset + xpos as f32 * xfrac, ypos)).unwrap();
	    } else {
		if y > last_y {
		    canvas.set_draw_color(Color::RGBA(0x00, 0x80 + ((((y - last_y) / 4.0) * 255.0) as u8), 0, 0x7f));
		} else {
		    canvas.set_draw_color(Color::RGBA(0x80 + (((last_y - y) / 4.0) * 255.0) as u8, 0, 0, 0x7f));
		}
		canvas.draw_fline(sdl2::rect::FPoint::new(xoffset + last_xpos as f32 * xfrac, last_ypos),
				  sdl2::rect::FPoint::new(xoffset + xpos as f32 * xfrac, ypos)).unwrap();
	    }

	    sample_within_tick += 1;
	    xpos += 1;
	    last_y = y;
	    last_ypos = ypos;
	    if sample_within_tick == tick_length {
		next_tick = tick + 1;
		sample_within_tick = 0;
	    }
	}
    }
}


fn play_song2(data : &datafiles::AmberstarFiles, song_nr : usize) -> Result<(), String> {
    let mut song = &data.songs[song_nr];
    let sdl_context = sdl2::init().unwrap();

    let audiocore = audio::acore::init(&sdl_context);
    let mut mixer = audiocore.mixer();
    let mut song_player = SongPlayerAudioSource::new(&data.sample_data, &data.songs, audiocore.frequency);
    let song_tracer = ArcDemoSongTracer::new();
    mixer.add_source(song_player.player());
    let mut poly_it = SongIterator::new(&song,
				    song.songinfo.first_division,
				    song.songinfo.last_division);


    // Graphics

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let mut scale : usize = 8;


    let window = video_subsystem.window("amber-remix", 3000, 2100)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();
    //let creator = canvas.texture_creator();

    let mut new_song_nr = Some(song_nr);
    let mut current_song_nr = song_nr;
    let mut start_tick: usize = 0;
    let mut following_tick = true;

    song_player.play(&poly_it);
    song_player.set_tracer(song_tracer.tracer());

    let font_size = FONT_SIZE;
    // --------------------------------------------------------------------------------
    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).unwrap();
    let font = Font::new_ttf(&ttf_context, "/usr/share/fonts/truetype/freefont/FreeMonoBold.ttf", font_size);
    // --------------------------------------------------------------------------------


    canvas.set_draw_color(Color::RGBA(0, 0, 0x40, 255));
    canvas.clear();
    canvas.present();

    let mut current_info_function: InfoFunction = songinfo_help;

    let info_functions: Vec<(Keycode, &str, InfoFunction)> = vec![
	(Keycode::F1, "Help", songinfo_help),
	(Keycode::F2, "Divisions", songinfo_divisions),
	(Keycode::F3, "Monopatterns", songinfo_monopatterns),
	(Keycode::F4, "Timbres", songinfo_timbres),
	(Keycode::F5, "Instruments", songinfo_instruments_samples),
	(Keycode::F6, "Channels", songinfo_channel_stat),
    ];

    let mut event_pump = sdl_context.event_pump().unwrap();
    'running: loop {
        canvas.set_draw_color(Color::RGBA(0, 0, 0x40, 0xff));
        canvas.clear();

	let waveform_pixel_width: u32 = 2800;

	if following_tick {
	    let tick_pixel_width = (mixer.sample_rate / (50 * scale as usize)) as u32;
	    let latest_tick = song_tracer.latest_tick() as u32;
	    if latest_tick * tick_pixel_width < waveform_pixel_width {
		start_tick = 0;
	    } else {
		start_tick = (latest_tick - (waveform_pixel_width / tick_pixel_width)) as usize;
	    }
	}

	let current_song_info = CurrentSongInfo {
	    song: &song,
	    tick: start_tick,
	    song_nr: current_song_nr,
	    info_functions: &info_functions,
	};

	song_tracer.draw_info(&mut canvas,
			      sdl2::rect::Rect::new(100, 10,
						    waveform_pixel_width, 450),
			      &font,
			      current_song_info,
			      current_info_function);

	for c in 0..4 {
	    song_tracer.draw_audio_track(&mut canvas,
					 sdl2::rect::Rect::new(100, 500 + (c as i32 * 400),
							       waveform_pixel_width, 256),
					 c,
					 start_tick,
					 scale as i32,
					 &font
	    );
	}

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
		    break 'running
                },
                Event::KeyDown { keycode : Some(kc), repeat:false, .. } => {
		    match kc {
			Keycode::BACKSPACE => { new_song_nr = Some(current_song_nr) },
			Keycode::F11 =>  { if current_song_nr > 0 { new_song_nr = Some(current_song_nr - 1); } },
			Keycode::F12 => { if current_song_nr < data.songs.len() - 1 { new_song_nr = Some(current_song_nr + 1); } },
			Keycode::Return => {},
			Keycode::SPACE  => { song_player.stop(); },
			Keycode::RIGHTBRACKET => { scale <<= 1 },
			Keycode::LEFTBRACKET => { if scale > 1 { scale >>= 1 } },
			Keycode::KP_4 => { following_tick = false;
					   start_tick = if start_tick < scale { 0 } else { start_tick - scale } },
			Keycode::KP_6 => { following_tick = false;
					    start_tick += scale },
			Keycode::KP_1 => { following_tick = false;
					   start_tick = if start_tick < 1 { 0 } else { start_tick - 1 } },
			Keycode::KP_3 => { following_tick = false;
					    start_tick += 1 },
			Keycode::KP_ENTER => { following_tick = true; },
			_ => {
			    for (k, _, f) in info_functions.iter() {
				if *k == kc {
				    current_info_function = *f;
				    break;
				}
			    }
			}
		    }
                },
                _ => {}
            }
        }

	if let Some(nr) = new_song_nr {
	    new_song_nr = None;
	    current_song_nr = nr;
	    song = &data.songs[nr];
	    println!("{}", song);
	    poly_it = SongIterator::new(&song,
					song.songinfo.first_division,
					song.songinfo.last_division);
	    song_player.play(&poly_it);
	    start_tick = 0;
	    following_tick = true;
	}

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 50));
    }

    Ok(())
}


fn play_song(data : &datafiles::AmberstarFiles, song_nr : usize) {
    let sdl_context = sdl2::init().unwrap();

    let mut audiocore = audio::init(&sdl_context);
    let mut mixer = audiocore.start_mixer(&data.sample_data.data[..]);
    let mut instr = InstrSelect {
	data, mixer:&mut mixer,
	song_nr : 0,
	sample_nr : 0,
	instrument_nr : 0,
	timbre_nr : 0,
	monopattern_nr : 0,
	mode : ISelect::Instrument };

    instr.song_nr = song_nr;
    instr.play_song();

    info!("Playing song {song_nr}");

    loop {
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
    //mixer.shutdown();
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
	    let dest = cli.output;

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
	    // "song-old"	=> {
	    // 	let source = &args[2];
	    // 	play_song(&data, str::parse::<usize>(source).unwrap());
	    // },
	    Command::Song{song:song_nr} =>
		play_song2(&data, song_nr.unwrap_or(0)).unwrap(),
	    // "iter-song"	=> {
	    // 	let source = &args[2];
	    // 	print_iter_song(&data, str::parse::<usize>(source).unwrap());
	    // },
	    // "debug-audio" => {
	    // debug_audio::debug_audio(&data).unwrap();
	    // },
	    Command::GfxDemo => show_images(&data),
	    Command::MapViewer => map_demo::show_maps(&data),
	    Command::Extract{..}  => {}, // already handled above
	}
    }

    Ok(())
}
