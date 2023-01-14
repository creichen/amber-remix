// Copyright (C) 2022, 23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::time::Duration;

use sdl2::{pixels::Color, event::Event, keyboard::Keycode, rect::{Rect, Point}, render::{TextureQuery, Canvas, Texture, TextureCreator}, video::Window, ttf::Sdl2TtfContext};

use crate::datafiles::{map, self, tile::Tileset, labgfx, pixmap::Pixmap};
use std::fmt::Write;

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

struct NPC {
    mapnpc : map::MapNPC,
    cycle_pos : usize,  // position in NPC cycle
    cycle_frac : u8, // fractional position in NPC cycle, in 1/256
}

impl NPC {
    pub fn new(mapnpc : map::MapNPC) -> NPC {
	NPC { mapnpc,
	      cycle_pos : 0,
	      cycle_frac : 0,
	}
    }

    pub fn cycle_len(&self) -> usize {
	match &self.mapnpc.movement {
	    map::NPCMovement::Cycle(cycle) => cycle.len(),
	    _ => 1,
	}
    }

    pub fn pos_at_cyclepos(&self, cycle_pos : usize) -> Option<(usize, usize)> {
	match &self.mapnpc.movement {
	    map::NPCMovement::Cycle(cycle) => cycle[cycle_pos % cycle.len()],
	    _                              => Some(self.mapnpc.start_pos),
	}
    }

    /// Move NPC forward in its cycle by cycle_frac_inc 1/256th steps
    pub fn advance_cycle(&mut self, cycle_frac_inc : usize) {
	let cycle_total_frac = (self.cycle_pos << 8) + (cycle_frac_inc + self.cycle_frac as usize);
	let cycle_total_frac_mod = cycle_total_frac % (self.cycle_len() << 8);
	self.cycle_pos = cycle_total_frac_mod >> 8;
	self.cycle_frac = (cycle_total_frac_mod & 0xff) as u8;
    }

    // Returns the floating point tile position (to allow easy positional scaling)
    pub fn tile_pos(&self) -> Option<(f32, f32)> {
	return self.tile_pos_at(self.cycle_pos, self.cycle_frac as usize);
    }

    pub fn tile_pos_at(&self, cycle_pos : usize, cycle_frac : usize) -> Option<(f32, f32)> {
	// To capture movement between tiles, first get the start tile position:
	let start_pos = self.pos_at_cyclepos(cycle_pos);
	let end_pos = self.pos_at_cyclepos(cycle_pos + 1);
	if start_pos == None && end_pos == None {
	    return None;
	} else if start_pos == None {
	    return end_pos.map(|(x,y)| (x as f32, y as f32));
	} else if end_pos == None {
	    return start_pos.map(|(x,y)| (x as f32, y as f32));
	}
	let (start_x, start_y) = start_pos.unwrap();
	// Now the end tile position
	let (end_x, end_y) = end_pos.unwrap();
	// Factor in how far we've moved:
	let end_weight = cycle_frac;
	let start_weight = 0x100 - cycle_frac;
	let x_256 = start_x * start_weight + end_x * end_weight;
	let y_256 = start_y * start_weight + end_y * end_weight;
	return Some((x_256 as f32 / 256.0,
		     y_256 as f32 / 256.0));
    }

    pub fn draw(&self, tiles : &Tileset<Texture<'_>>,
		canvas : &mut Canvas<sdl2::video::Window>,
		anim_index : usize) {
	if let Some((x, y)) = self.tile_pos() {
	    let xpos = (x * 32.0) as i32;
	    let ypos = (y * 32.0) as i32;
	    draw_tile(tiles, canvas,
		      self.mapnpc.sprite, xpos, ypos, anim_index);
	}
    }
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

fn labblock_textures<'a, T>(data : &datafiles::AmberstarFiles, tc : &'a TextureCreator<T>,
			    labdata : &labgfx::LabData) -> Vec<labgfx::LabBlock<Texture<'a>>> {
    let palette = &data.palettes[labdata.magic_7[6] as usize - 1];
    let labblocks = &data.labgfx.labblocks;
    // let pallettized : Vec<labgfx::LabBlock<Pixmap>> = labdata.labblocks.iter().map(|n| {pwarn!("flattening {}", *n); labblocks[*n].flatten().with_palette(palette)}).collect();
    let mut pallettized = vec![];
    for n in labdata.labblocks.iter() {
	pwarn!("flattening {}", *n);
	// WIP:
	let r = labblocks[*n].flatten().with_palette(palette);
	pallettized.push(r);
    }
    return pallettized.iter().map(|l| l.as_textures(&tc)).collect();
}


pub fn show_maps(data : &datafiles::AmberstarFiles) {
    map::debug_summary();

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem.window("amber-remix", 2560, 1440)
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
    let white = Color::RGBA(0xff, 0xff, 0xff, 0xff);
    let npc_col = Color::RGBA(0, 0xff, 0, 0xff);
    let event_col = Color::RGBA(0xff, 0, 0, 0xff);
    let tileinfo_col = Color::RGBA(0, 0xaf, 0xff, 0xff);
    let help : Vec<(Color, String)> = vec![
	"=== key bindings ===",
	"[F7] toggle tile nr printing",
	"[F8] toggle event info",
	"[F9] toggle NPC info",
	"[F10] toggle NPC routes",
	"<- [F11] map [F12] ->",
	" ",
    ].iter().map(|s| (white, s.to_string())).collect();

    let mut map_nr = 0x40; // Twinlake Graveyard, the starting map

    let mut lab_palette_nr = 0;
    let mut lab_nr = 0;
    let mut draw_npc_routes = true;
    let mut draw_npc_info = true;
    let mut draw_event_info = true;
    let mut draw_tile_nr = false;

    // WIP
    //let labblocks : Vec<Vec<labgfx::LabBlock<Texture>>> = data.labgfx.labdata.iter().map(|labdata| labblock_textures(&data, &creator, labdata)).collect();
    let labblocks : Vec<Vec<labgfx::LabBlock<Texture>>> = vec![labblock_textures(&data, &creator, &data.labgfx.labdata[0])];


    'running: loop {

	let map = &data.maps[map_nr];
	let lab_info = &data.labgfx.labdata[map.tileset];

	let width = map.width;
	let height = map.height;

	let tileset = usize::min(1, data.maps[map_nr].tileset); // tileset for 3d maps = background image

	let mut npcs : Vec<NPC> = map.npcs.iter().map(|x| NPC::new(x.clone())).collect();

	let labblock = data.labgfx.labblocks[lab_nr].with_palette(&data.palettes[lab_palette_nr]).as_textures(&creator);

	// Run the loop below while the current map is selected
	let mut i : usize = 0;
	'current_map: loop {
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
			Keycode::F2           => { if lab_nr > 0 { lab_nr -= 1; break 'current_map; } },
			Keycode::F3           => { if lab_nr < data.labgfx.labblocks.len() - 1 { lab_nr += 1; break 'current_map; } },
			Keycode::F4           => { if lab_palette_nr > 0 { lab_palette_nr -= 1; break 'current_map; } },
			Keycode::F5           => { if lab_palette_nr < data.palettes.len() - 1 { lab_palette_nr += 1; break 'current_map; } },
			Keycode::F7           => { draw_tile_nr = !draw_tile_nr; },
			Keycode::F8           => { draw_event_info = !draw_event_info; },
			Keycode::F9           => { draw_npc_info = !draw_npc_info; },
			Keycode::F10          => { draw_npc_routes = !draw_npc_routes; },
			Keycode::F11          => { if map_nr > 0 { map_nr -= 1; break 'current_map; } },
			Keycode::F12          => { if map_nr < data.maps.len() - 1 { map_nr += 1; break 'current_map; } },
			_                     => {},
		    }
		},
                    _ => {}
		}
            }

	    let mut current_help = help.clone();

            i = i + 1;
            canvas.set_draw_color(Color::RGB(20, 20, 20));
            canvas.clear();

	    for map_index in 0..map.num_layers {
		for y in 0..height {
		    for x in 0..width {
			let tile = map.tile_at(map_index, x, y);

			let xpos = (x as i32) * 32;
			let ypos = (y as i32) * 32;

			if let Some(tile_id) = tile {
			    draw_tile(&tile_textures[tileset],
				      &mut canvas,
				      tile_id,
				      xpos, ypos, i >> 4);
			    if draw_tile_nr {
				let msg = format!("{:02x}", tile_id);
				font.draw_to_with_outline(&mut canvas, &msg,
							  xpos as isize, ypos as isize + 16,
							  tileinfo_col,
							  Color::RGBA(0, 0, 0, 0x7f));
			    }
			}
		    }
		}
	    }

	    // draw NPC
	    for npc in &mut npcs {
		npc.advance_cycle(0x64);
		npc.draw(&tile_textures[tileset],
			 &mut canvas,
			 i >> 4);
	    }


	    if draw_event_info {
		for y in 0..height {
		    for x in 0..width {
			let xpos = (x as isize) * 32;
			let ypos = (y as isize) * 32;

			if let Some(hotspot_id) = map.hotspot_at(x, y) {
			    // draw text on hotspots
			    let icon_nr_str = format!("{:02x}", hotspot_id);
			    font.draw_to_with_outline(&mut canvas, &icon_nr_str,
						      xpos, ypos,
						      event_col,
						      Color::RGBA(0, 0, 0, 0x7f),
			    );
			}
		    }
		}
	    }

	    // draw info about NPCs
	    let mut npc_nr = 0;
	    for npc in &mut npcs {
		if draw_npc_info {
		    if let Some((x, y)) = npc.tile_pos() {
		    let nr_str = format!("{:02x}", npc_nr);
			font.draw_to_with_outline(&mut canvas, &nr_str,
						  (x * 32.0) as isize, (y * 32.0) as isize,
						  npc_col,
						  Color::RGBA(0, 0, 0, 0x3f),
			);
		    }

		    let action = match npc.mapnpc.talk_action {
			map::NPCAction::PopupMessage(msg) => format!("message: \"{}\"", data.map_text[map_nr].strings[msg]),
			map::NPCAction::Chat(identity)    => format!("{} with {:x}", if npc.mapnpc.hostile() {"fight"} else {"chat"}, identity),
		    };
		    let info = format!("NPC {:02x}: flags {:x} {}",
				       npc_nr, npc.mapnpc.flags, action);

		    current_help.push((npc_col, info));
		}

		if draw_npc_routes {
		    canvas.set_draw_color(Color::RGBA(0, 0xff, 0, 0x80));

		    for i in 0..npc.cycle_len() {
			if let Some((x, y)) = npc.pos_at_cyclepos(i) {
			    if let Some((x_next, y_next)) = npc.pos_at_cyclepos(i + 1) {
				if x != x_next || y != y_next {
				    canvas.draw_line(Point::new(((x * 2 + 1) * 16) as i32,
								((y * 2 + 1) * 16) as i32),
						     Point::new(((x_next * 2 + 1) * 16) as i32,
								((y_next * 2 + 1) * 16) as i32)).unwrap();
				}
			    } else {
				// NPC disappears here
				canvas.draw_line(Point::new((x * 32) as i32 + 8,
							    (y * 32) as i32 + 8),
						 Point::new((x * 32) as i32 + 24,
							    (y * 32) as i32 + 24)).unwrap();
				canvas.draw_line(Point::new((x * 32) as i32 + 8,
							    (y * 32) as i32 + 24),
						 Point::new((x * 32) as i32 + 24,
							    (y * 32) as i32 + 8)).unwrap();
			    }
			}
		    }
		}
		npc_nr += 1;
	    }

	    if draw_event_info {
		for (i, e) in map.event_table.iter().enumerate() {
		    let mut msg = "".to_string();
		    for b in &e.raw {
			write!(msg, " {b:02x}").unwrap();
		    }
		    current_help.push((event_col, format!("ev[{:02x}] ={msg}", i+1)));
		}
	    }

	    if draw_tile_nr {
		if map.first_person {
		    current_help.push((tileinfo_col, format!("LAB_INFO.AMB.{:04} magic1:{:x} magic7:{:x?} blocks={:x?}", map.tileset,
							     lab_info.magic_byte, lab_info.magic_7, lab_info.labblocks)));
		}
		for (i, l) in map.lab_info.iter().enumerate() {
		    let img_id = lab_info.labblocks[i];
		    current_help.push((tileinfo_col, format!("labblock[{:02x}] = {:02x}{:02x}{:02x}{:02x} {:02x} {:02x} {:02x}  img={:02x}",
							     i + 1,
							     l.head[0],
							     l.head[1],
							     l.head[2],
							     l.head[3],
							     l.rest[0],
							     l.rest[1],
							     l.rest[2],
							     img_id)));
		}
	    }

	    // labblock
	    {
		current_help.push((white, format!("LAB: block {lab_nr}={lab_nr:#x}, pal {lab_palette_nr}, fmt {:?}, dim {}x{}", labblock.block_type, labblock.images.len(), labblock.num_frames_distant)));
		// current_help.push((white, format!("LAB: ?| {:?}", &labblock.unknowns[0..labblock.unknowns.len() >> 1])));
		// current_help.push((white, format!("LAB: ?| {:?}", &labblock.unknowns[labblock.unknowns.len() >> 1..])));
	    }

	    font.draw_to(&mut canvas, format!("Map {} ({:#02x}): {}", map_nr, map_nr, map.name).as_str(),
			 1680, 10, Color::RGBA(0xaf, 0xaf, 0xaf, 0xff));
	    let mut ypos = 20 + font_size;
	    for (help_col, help_line) in &current_help {
		ypos += font_size + 4;
		font.draw_to(&mut canvas, help_line,
			     1650, ypos as isize, *help_col);
	    }

	    ypos += 20;

	    //for (row_nr, row) in labblock.images.iter().enumerate() {
	    for (row_nr, row) in labblocks[0][lab_nr].images.iter().enumerate() {
		let mut xpos = 1020;
		let mut maxheight = 0;
		//for (column_nr, _pixmap) in row.pixmaps.iter().enumerate() {
		for (column_nr, column) in row.pixmaps.iter().enumerate() {
		    //WIP: this is what we used to do
		    //let column = &pixmap.pixmap;

		    // WIP: current test:
		    // let lba = &labblocks[0][lab_nr];
		    // if column_nr >= lba.images.len() {
		    // 	break;
		    // }
		    // let lbr = &lba.images[column_nr];
		    // if row_nr >= lbr.pixmaps.len() {
		    // 	break;
		    // }
		    // let column = &lbr.pixmaps[row_nr].pixmap;
		    let column = &column.pixmap;

		    // WIP continues "normally" below:
		    let TextureQuery { width, height, .. } = column.query();
		    canvas.set_draw_color(Color::RGBA(0xff, 0xff, 0, 0xff));
		    canvas.draw_rect(Rect::new(xpos as i32, ypos as i32, 2 + width * 2 as u32, 2 + height * 2 as u32)).unwrap();
		    canvas.copy(&column,
				Rect::new(0, 0, width as u32, height as u32),
				Rect::new(1 + xpos as i32, 1 + ypos as i32, width * 2 as u32, height * 2 as u32)).unwrap();

		    font.draw_to(&mut canvas, format!("{column_nr},{row_nr}").as_str(),
				 xpos, (ypos + (height as usize) * 2 + 2) as isize, Color::RGBA(0xff, 0xff, 0, 0xff));

		    xpos += (width * 2 + 4) as isize;
		    maxheight = u32::max(maxheight, height * 2);
		}

		ypos += (maxheight + 24) as usize;
	    }

            canvas.present();
            ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
	}
    }
}
