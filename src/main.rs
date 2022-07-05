//#[macro_use(lazy_static)]
//extern crate lazy_static;
use std::{io, time::Duration, env, sync::{Arc, Mutex}, collections::VecDeque};

use audio::{Mixer, AQOp};
use datafiles::music;
use sdl2::{pixels::Color, event::Event, keyboard::Keycode, rect::Rect, audio::AudioSpecDesired};

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

struct InstrSelect<'a> {
    data : &'a datafiles::AmberStarFiles,
    mixer : &'a mut Mixer,
    song_nr   : usize,
    sample_nr : usize,
}

impl<'a> InstrSelect<'a> {
    fn move_sample(&mut self, dir : isize) {
	self.sample_nr = (((self.sample_nr + self.num_samples()) as isize + dir) as usize) % self.num_samples();
	self.print_config();
    }
    fn move_song(&mut self, dir : isize) {
	self.song_nr = (((self.song_nr + self.num_songs()) as isize + dir) as usize) % self.num_songs();
	if self.sample_nr > self.num_samples() {
	    self.sample_nr = self.num_samples() - 1;
	}
	self.print_config();
    }
    fn num_samples(&self) -> usize { self.data.songs[self.song_nr].basic_samples.len() }
    fn num_songs(&self)   -> usize { self.data.songs.len() }
    fn play(&mut self, note : usize) {
	let sampleinfo = self.data.songs[self.song_nr].basic_samples[self.sample_nr];
	let sample = AQOp::from(sampleinfo);
	let period = music::PERIODS[note];
	let freq = music::period_to_freq(period);
	println!(" .. playing {sampleinfo} at freq {freq}");
	self.mixer.set_iterator(audio::make_note(freq, sample, 1000));
    }
    fn print_config(&self) {
	println!("Switched to: Song {}/{}, instrument {}/{}", self.song_nr, self.num_songs(), self.sample_nr, self.num_samples());
    }
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
    let mut instr = InstrSelect { data, mixer:&mut mixer, song_nr : 0, sample_nr : 0 };

    canvas.set_draw_color(Color::RGB(0, 255, 255));
    canvas.clear();
    canvas.present();
    let mut event_pump = sdl_context.event_pump().unwrap();
    let mut i = 0;
    'running: loop {
        i = (i + 1) % 255;
        canvas.set_draw_color(Color::RGB(i, 64, 255 - i));
        canvas.clear();

	for j in 0..data.pics80.len() {
	    let img = &data.pics80[j];
	    let creator = canvas.texture_creator();
	    let texture = img.as_texture(&creator);
	    canvas.copy(&texture, None, Some(Rect::new(j as i32 * (img.width as i32 + 8), 0, img.width, img.height))).unwrap();
	}

	let img = &data.pic_intro;
	let creator = canvas.texture_creator();
	let texture = &data.pic_intro.as_texture(&creator);
	canvas.copy(&texture, None, Rect::new(100, 200, img.width, img.height)).unwrap();

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
		    break 'running
                },
                Event::KeyDown { keycode : Some(kc), .. } => {
		    match kc {
			Keycode::Num1 => instr.move_song(-1),
			Keycode::Num2 => instr.move_song(1),
			Keycode::Num3 => instr.move_sample(-1),
			Keycode::Num4 => instr.move_sample(1),
			Keycode::A => instr.play(12),
			Keycode::W => instr.play(13),
			Keycode::S => instr.play(14),
			Keycode::E => instr.play(15),
			Keycode::D => instr.play(16),
			    _ => { println!("<ESC>: quit; 1/2 : fwd/backwd song, 3/4: fwd/backwd instrument, asd... -> play note")},
		    }
                },
                _ => {}
            }
        }
        // The rest of the game loop goes here...

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}

// ================================================================================
fn main() -> io::Result<()> {
    env_logger::init();
    let data = datafiles::AmberStarFiles::load("data");

    let args : Vec<String> = env::args().collect();

    if args.len() == 2 {
	if args[1] == "strings" {
	    print_strings(&data);
	} else {
	    show_images(&data);
	}
    }

    Ok(())
}

// fn oldmain() -> io::Result<()> {
//     let args : Vec<String> = env::args().collect();
//     let source = &args[1];
//     let mut df = datafiles::DataFile::load(&source);
//     println!("File type: {}", df.filetype);
//     for i in 0..df.num_entries {
// 	println!("Extracting {i}/{}", df.num_entries);
// 	let data = df.decode(i);
// 	let filename = format!("decompressed/{source}.{:04}", i);
// 	println!("  -> writing {} bytes to {filename}", data.len());
// 	fs::write(filename, data).expect("Unable to write file");
//     }
//     Ok(())
// }
