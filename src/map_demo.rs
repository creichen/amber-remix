// Copyright (C) 2022, 23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::time::Duration;

use sdl2::{pixels::Color, event::Event, keyboard::Keycode, rect::Rect, render::{TextureQuery, Canvas, Texture}, video::Window, ttf::Sdl2TtfContext};

use crate::datafiles::{map, self, tile::Tileset};

fn draw_tile(tiles : &Tileset<Texture<'_>>,
	     canvas : &mut Canvas<sdl2::video::Window>,
	     tile : usize, xpos : i32, ypos : i32, anim_index : usize) {
    const SRC_WIDTH : usize = 16;
    const SRC_HEIGHT : usize = 16;
    const WIDTH : usize = 32;
    const HEIGHT : usize = 32;
    if tile > 0 && tile <= tiles.tile_icons.len() {
	let frames = &tiles.tile_icons[tile - 1].frames;
	let index = anim_index % frames.len();

	canvas.copy(&frames[index],
		    Rect::new(0, 0, SRC_WIDTH as u32, SRC_HEIGHT as u32),
		    Rect::new(xpos as i32, ypos as i32, WIDTH as u32, HEIGHT as u32)).unwrap();
    }
}

struct Font<'a> {
    font : sdl2::ttf::Font<'a, 'a>,
}

impl<'a> Font<'a> {
    pub fn new_ttf(ttf_context : &'a Sdl2TtfContext, path : &str, size : usize) -> Font<'a> {
	// TODO: include font or use the existing one
	let mut font = ttf_context.load_font(path, size as u16).unwrap();
	font.set_style(sdl2::ttf::FontStyle::NORMAL);
	Font {
	    font
	}
    }

    pub fn draw_to(&self, canvas : &mut Canvas<Window>, text : &str, x : isize, y : isize, color : Color) {
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
    }
    pub fn draw_to_with_outline(&self, canvas : &mut Canvas<Window>, text : &str, x : isize, y : isize, color : Color, outline_color : Color) {
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
    }
}

pub fn show_maps(data : &datafiles::AmberStarFiles) {
    map::debug_summary();

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem.window("amber-remix", 2560, 1600)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();

    let creator = canvas.texture_creator();
    let tile_textures = vec![
	data.tiles[0].as_textures(&creator),
	data.tiles[1].as_textures(&creator),
	];

    let mut event_pump = sdl_context.event_pump().unwrap();

    let font_size = 14;

    // --------------------------------------------------------------------------------
    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).unwrap();
    let font = Font::new_ttf(&ttf_context, "/usr/share/fonts/truetype/freefont/FreeMonoBold.ttf", font_size);
    // --------------------------------------------------------------------------------
    let help : Vec<&str> = vec![
	"=== key bindings ===",
	"<- [F11] map [F12] ->",
    ];

    let mut map_nr = 0x41;

    'running: loop {

	let map = &data.maps[map_nr];

	let width = map.width;
	let height = map.height;

	let tileset = usize::min(1, data.maps[map_nr].tileset); // tileset for 3d maps = background image

	// Run the loop below while the current map is selected
	let mut i : usize = 0;
	'current_map: loop {
            i = i + 1;
            canvas.set_draw_color(Color::RGB(20, 20, 20));
            canvas.clear();

	    for map_index in 0..map.num_layers {
		for y in 0..height {
		    for x in 0..width {
			let tile = map.tile_at(map_index, x, y);

			let xpos = (x as i32) * 32;
			let ypos = (y as i32) * 32;

			if let Some(icon) = tile {
			    draw_tile(&tile_textures[tileset],
				      &mut canvas,
				      icon,
				      xpos, ypos, i >> 4);
			}
		    }
		}
	    }
	    for y in 0..height {
		for x in 0..width {
		    let xpos = (x as isize) * 32;
		    let ypos = (y as isize) * 32;

		    if let Some(hotspot_id) = map.hotspot_at(x, y) {
			// draw text on hotspots
			let icon_nr_str = format!("{:02x}", hotspot_id);
			font.draw_to_with_outline(&mut canvas, &icon_nr_str,
						  xpos, ypos,
						  Color::RGBA(0xff, 0, 0, 0xff),
						  Color::RGBA(0, 0, 0, 0x7f),
			);
		    }
		}
	    }

            for event in event_pump.poll_iter() {
		match event {
                    Event::Quit {..} |
                    Event::KeyDown {
			keycode: Some(Keycode::Escape), .. } => {
			break 'running;
                    },
                Event::KeyDown { keycode : Some(kc), repeat:false, .. } => {
		    match kc {
			Keycode::F1           => {},
			Keycode::F11          => { if map_nr > 0 { map_nr -= 1; break 'current_map; } },
			Keycode::F12          => { if map_nr < data.maps.len() - 1 { map_nr += 1; break 'current_map; } },
			_                     => {},
		    }
		},
                    _ => {}
		}
            }

	    font.draw_to(&mut canvas, format!("Map {} ({:#02x}): {}", map_nr, map_nr, map.name).as_str(),
			 2000, 10, Color::RGBA(0xaf, 0xaf, 0xaf, 0xff));
	    let mut ypos = 20 + font_size;
	    for help_line in &help {
		ypos += font_size + 4;
		font.draw_to(&mut canvas, help_line,
			     2200, ypos as isize, Color::RGBA(0xff, 0xff, 0xff, 0xff));
	    }

            canvas.present();
            ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
	}
    }
}
