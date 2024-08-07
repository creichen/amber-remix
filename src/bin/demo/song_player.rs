// Copyright (C) 2024 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use amber_remix::audio::iterator::{AQOp, AudioIterator};
use crate::font::{Font, FONT_SIZE};

use sdl2::video::Window;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use amber_remix::audio::amber::SongIterator;
use amber_remix::datafiles::music::Song;
use sdl2::{pixels::Color, event::Event, keyboard::Keycode, rect::Rect, render::Canvas};

use amber_remix::audio::experiments::{SongPlayerAudioSource, SongTracer};
use amber_remix::datafiles::{self};
use amber_remix::audio::{self};


pub fn print_iter_song(data : &datafiles::AmberstarFiles, song_nr : usize) {
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

// const SAMPLE_RATE : usize = audio::experiments::SAMPLE_RATE;

// fn float_to_i16(x: f32) -> i16 {
//     if x > 1.0 { 0x3fff } else
//     { if x < -1.0 { -0x4000 } else { (x * 32767.0) as i16 }}
// }

// fn float_buffers_merge_to_i16(input_l : &[f32], input_r: &[f32]) -> Vec<i16> {
//     let mut result = Vec::new();
//     for xr in 0..input_l.len() {
// 	result.push(float_to_i16(input_l[xr]));
// 	result.push(float_to_i16(input_r[xr]));
//     }
//     result
// }

// --------------------------------------------------------------------------------

type InfoFunction = fn(&mut PaginatedWriter, &ArcDemoSongTracer, CurrentSongInfo) -> ();

struct CurrentSongInfo<'a> {
    song: &'a Song,
    tick: usize,
    song_nr: usize,
    info_functions: &'a [(Keycode, &'a str, InfoFunction)],
    name: &'a str,
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
		      _highlight_color: Color) {
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
	pw.print("[F1] for help  ");
	pw.println(&format!("Song {:02x}: {}", current_song_info.song_nr, current_song_info.name));
	pw.set_color(COLOR_YELLOW);
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


pub fn play_song(data : &datafiles::AmberstarFiles, song_nr : usize) -> Result<(), String> {
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
	    name: &data.amberdev.song_names[current_song_nr],
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
