#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

#[macro_use(lazy_static)]
extern crate lazy_static;

use std::{time::Duration, io, env, fs, path::Path};

use audio::{Mixer, AQOp, SampleRange};
use datafiles::music::{BasicSample, Song};
use sdl2::{pixels::Color, event::Event, keyboard::Keycode, rect::Rect, render::Canvas};

use crate::audio::amber;

mod datafiles;
mod audio;

fn print_strings(data : &datafiles::AmberStarFiles) {

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

enum ISelect {
    Sample,
    Instrument,
    Timbre,
    Monopattern,
}

struct InstrSelect<'a> {
    data : &'a datafiles::AmberStarFiles,
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

    canvas.set_draw_color(Color::RGB(255, 0, 128));
    canvas.draw_line(sdl2::rect::Point::new(startx -3, ybase),
		     sdl2::rect::Point::new(startx -3, ybase - 25)).unwrap();
    canvas.draw_line(sdl2::rect::Point::new(startx + (x / xfactor) +3, ybase),
		     sdl2::rect::Point::new(startx + (x / xfactor) +3, ybase - 25)).unwrap();
}

fn show_images(data : &datafiles::AmberStarFiles) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem.window("amber-blasphemy", 3000, 1600)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();

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

    canvas.set_draw_color(Color::RGB(0, 255, 255));
    canvas.clear();
    canvas.present();
    let mut event_pump = sdl_context.event_pump().unwrap();
    let mut i = 0;
    'running: loop {
        i = (i + 1) % 255;
        canvas.set_draw_color(Color::RGB(i, 64, 128 - (i>>1)));
        canvas.clear();

	for j in 0..data.pics80.len() {
	    let img = &data.pics80[j];
	    let creator = canvas.texture_creator();
	    let texture = img.as_texture(&creator);
	    canvas.copy(&texture, None, Some(Rect::new(j as i32 * (img.width as i32 + 8), 0, img.width, img.height))).unwrap();
	}

	// let img = &data.pic_intro;
	// let creator = canvas.texture_creator();
	// let texture = &data.pic_intro.as_texture(&creator);
	// canvas.copy(&texture, None, Rect::new(100, 200, img.width, img.height)).unwrap();

	let sampledata = instr.basicsample();

	canvas.set_draw_color(Color::RGB(150, 255, 0));
	draw_sampledata(&data.sample_data.data[..], &mut canvas, 300, sampledata.attack);
	if let Some(sustain) = sampledata.looping {
	    canvas.set_draw_color(Color::RGB(1, 255, 0));
	    draw_sampledata(&data.sample_data.data[..], &mut canvas, 500, sustain);
	}

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
		    break 'running
                },
                Event::KeyDown { keycode : Some(kc), repeat:false, .. } => {
		    match kc {
			Keycode::LeftBracket  => instr.move_song(-1),
			Keycode::RightBracket => instr.move_song(1),
			Keycode::Minus        => instr.move_sample(-1),
			Keycode::Equals       => instr.move_sample(1),
			Keycode::Quote        => instr.move_instrument(-1),
			Keycode::Backslash    => instr.move_instrument(1),
			Keycode::Period       => instr.move_timbre(-1),
			Keycode::Slash        => instr.move_timbre(1),
			Keycode::Kp7          => instr.move_monopattern(-1),
			Keycode::Kp9          => instr.move_monopattern(1),

			Keycode::Return       => instr.play_song(),
			Keycode::Space        => instr.play(0),
			Keycode::Z            => instr.play(12),
			Keycode::S            => instr.play(13),
			Keycode::X            => instr.play(14),
			Keycode::D            => instr.play(15),
			Keycode::C            => instr.play(16),
			Keycode::V            => instr.play(17),
			Keycode::G            => instr.play(18),
			Keycode::B            => instr.play(19),
			Keycode::H            => instr.play(20),
			Keycode::N            => instr.play(21),
			Keycode::J            => instr.play(21),
			Keycode::M            => instr.play(23),

			Keycode::Q            => instr.play(24),
			Keycode::Num2         => instr.play(25),
			Keycode::W            => instr.play(26),
			Keycode::Num3         => instr.play(27),
			Keycode::E            => instr.play(28),
			Keycode::R            => instr.play(29),
			Keycode::Num5         => instr.play(30),
			Keycode::T            => instr.play(31),
			Keycode::Num6         => instr.play(32),
			Keycode::Y            => instr.play(33),
			Keycode::Num7         => instr.play(34),
			Keycode::U            => instr.play(35),

			Keycode::I            => instr.play(36),
			Keycode::Num9         => instr.play(37),
			Keycode::O            => instr.play(38),
			Keycode::Num0         => instr.play(39),
			Keycode::P            => instr.play(40),
			    _ => { println!("<ESC>: quit\n [/] : song\n -|=: sample\n '|\\: instrument\n .|/: timbre\n  Num7/Num9: Monopattern\nzsxdc.../q2w3e... -> play note; Space: play zero note (monopatterns)")},
		    }
                },
                _ => {}
            }
        }
        // The rest of the game loop goes here...

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
    mixer.shutdown();
}


fn play_song(data : &datafiles::AmberStarFiles, song_nr : usize) {
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
    let data = datafiles::AmberStarFiles::load("data");
    let args : Vec<String> = env::args().collect();

    if args.len() >= 2 {
	if args[1] == "strings" {
	    print_strings(&data);
	} else if args[1] == "song" {
	    let source = &args[2];
	    play_song(&data, str::parse::<usize>(source).unwrap());
	} else if args[1] == "extract" {
	    let source = &args[2];
	    let mut df = datafiles::DataFile::load(Path::new(source));
	    println!("File type: {}", df.filetype);
	    for i in 0..df.num_entries {
		println!("Extracting {i}/{}", df.num_entries);
		let data = df.decode(i);
		let filename = format!("decompressed/{source}.{:04}", i);
		println!("  -> writing {} bytes to {filename}", data.len());
		fs::write(filename, data).expect("Unable to write file");
	    }
	} else {
	    show_images(&data);
	}
    }

    Ok(())
}

