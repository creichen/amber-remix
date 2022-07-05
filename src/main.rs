#[macro_use(lazy_static)]
extern crate lazy_static;
use std::{io, time::Duration, env, sync::{Arc, Mutex}, collections::VecDeque};

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

struct D {}
impl audio::AudioIterator for D {
    fn next(&mut self, queue : &mut VecDeque<audio::AQOp>) {
	queue.push_back(audio::AQOp::SetVolume(0.05));
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

    let audio = sdl_context.audio().unwrap();

    let requested_audio = AudioSpecDesired {
	freq: Some(44100),
	channels: Some(2),
	samples: None
    };

    let mixer = audio::new(data.sample_data.data.clone());

    let device = audio.open_playback(None, &requested_audio, |spec| {
	return mixer.init(spec);
	//return mixer;
    }).unwrap();
    device.resume();
    let d = Arc::new(Mutex::new(D{}));
    mixer.set_channel(audio::CHANNELS[0], d);

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
