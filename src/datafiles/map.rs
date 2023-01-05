// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use crate::datafiles::{amber_string, decode, bytepattern::BPInfer};
use std::fmt::Write;

// ----------------------------------------
struct MapLayer<T> {
    width : usize,
    height : usize,
    tiles : Vec<T>,
}

impl<T : Clone + Copy> MapLayer<T> {
    fn new(width:usize, height: usize, data : &[T]) -> MapLayer<T> {
	MapLayer {
	    width, height,
	    tiles: data.to_vec(),
	}
    }

    fn get(&self, x : usize, y : usize) -> T {
	assert!(x < self.width);
	assert!(y < self.height);
	return self.tiles[self.width * y + x];
    }
}

// ----------------------------------------
// unsafe debugging code

static mut EVENT_OPS_INCOMPLETE : u32 = 0x00; /// flag for event ops that we failed to fully decode
static mut EVENT_OPS_PARAMETERS : Vec<Vec<BPInfer>> = vec![]; /// collect information about event ops

unsafe fn event_debug_summary() {
    for op in 1..0x18 {
	let completed = if 0 == (EVENT_OPS_INCOMPLETE & 1 << op) {
	    "  complete"
	} else {
	    "INCOMPLETE"
	};
	print!("{completed} event op type {op:02x} {}\n", BPInfer::explain_vec(&EVENT_OPS_PARAMETERS[op]));
    }
}

pub fn debug_summary() {
    unsafe {
	event_debug_summary();
    }
}

// ----------------------------------------
// Map layer elements
type TileIndex = Option<u8>;
type EventIndex = usize;

// ----------------------------------------
// Event is a DSL for describing actions
// attached to map events

type Keyword = usize;
type ImageIndex = usize;
type MapMessageIndex = usize; // Map string index
type MapIndex = usize;
type ChestIndex = usize; // number of CHESTDATA.AMB entry
type ChestFlagID = usize; // flag to store whether chest has been emptied

enum EventOp {
    LockedDoor(usize), // lock pick difficulty
    PopupMessage(Option<ImageIndex>, MapMessageIndex),
    LearnKeyword(Keyword),
    Teleport(usize, usize, Option<MapIndex>), // MapIndex is None if teleport is on same map
    ChestAccess(ChestIndex, ChestFlagID, MapMessageIndex),
    RestoreLP, // restore all life points
    RestoreSP, // restore all spell points
    WinGame,       // win game
}

enum EventCondition {
    Enter,
    Look
}

struct Event {
    raw: [u8;10],
    cond : EventCondition,
    program : Vec<EventOp>,
}

impl Event {
    fn new(data : &[u8]) -> Option<Event> {
	const OP_TELEPORT	: u8 = 0x01;
	const OP_LOCKED_DOOR	: u8 = 0x02;
	const OP_MESSAGE	: u8 = 0x03;
	const OP_CHEST		: u8 = 0x04;
	const OP_5		: u8 = 0x05;
	const OP_6		: u8 = 0x06; // ?? encounter
	const OP_7		: u8 = 0x07;
	const OP_8		: u8 = 0x08; // ?? continuous message
	const OP_RESTORE_LP	: u8 = 0x0b; // data[4]: message
	const OP_RESTORE_SP	: u8 = 0x0c; // data[4]: message
	const OP_3D_STORE	: u8 = 0x12; // ?? Only used in first-person view, probably stores/guilds
	const OP_3D_BARRIER     : u8 = 0x13; // ?? need crowbar to get through?
	const OP_3D_LOCKED_DOOR	: u8 = 0x14; // ?? Only used in first-person view
	// e.g., door to Family Home:   14 01 00 00 1c 04 00 97 00 00
	const OP_WIN_GAME	: u8 = 0x17; // Last OP

	if data[0] == 1 && data[1] == 0 && data[2] == 0 && data[7] == 0 {
	    // This would be "teleport nowhere"
	    None
	} else {
	    let mut notes = String::new();
	    let mut may_be_nonzero = 0x0000;

	    let cond = match data[0] {
		OP_MESSAGE => {
		    match data[3] {
			0 => EventCondition::Look,
			1 => EventCondition::Enter,
			_ => {
			    write!(notes, "\tUnknown condition for Message: {:02x}\n", data[3]).unwrap();
			    EventCondition::Look},
		    }
		}
		OP_CHEST  => EventCondition::Look,

		// default
		_ => EventCondition::Enter
	    };

	    let program = match data[0] {
		OP_TELEPORT	=> {
		    may_be_nonzero = (0x1 << 0)
			| (0x1 << 1)
			| (0x1 << 2)
			| (0x1 << 7);
		    vec![EventOp::Teleport(data[1] as usize, data[2] as usize,
					     if data[7] == 0 { None } else { Some((data[7] - 1) as MapIndex) })]
		}
		OP_LOCKED_DOOR	=> {
		    may_be_nonzero = (0x1 << 0)
			| (0x1 << 1); // lock difficulty?
		    vec![EventOp::LockedDoor(data[1] as usize)]
		},
		OP_MESSAGE	=> {
		    let image = if data[1] == 0 { None } else { Some((data[1] - 1) as ImageIndex) };
		    let mut result = vec![EventOp::PopupMessage(image, data[2] as MapMessageIndex)];
		    let keyword = decode::u16(data, 6);
		    if keyword != 0 {
			result.push(EventOp::LearnKeyword(keyword as Keyword));
		    }
		    may_be_nonzero = (0x1 << 0)
			| (0x1 << 1)
			| (0x1 << 2)
			| (0x1 << 3)
			| (0x1 << 6)
			| (0x1 << 7);
		    result
		},
		OP_RESTORE_SP |
		OP_RESTORE_LP => {
		    may_be_nonzero = (0x1 << 0)
			| (0x1 << 4);
		    vec![EventOp::PopupMessage(None, data[4] as MapMessageIndex),
			 if data[0] == OP_RESTORE_SP { EventOp::RestoreSP } else { EventOp::RestoreLP } ]
		},
		OP_WIN_GAME => {
		    may_be_nonzero = 0x1 << 0;
		    vec![EventOp::WinGame]
		},
		OP_CHEST	=> {
		    may_be_nonzero = (0x1 << 0)
			| (0x1 << 5)
			| (0x1 << 7)
			| (0x1 << 9);
		    vec![EventOp::ChestAccess(data[7] as ChestIndex, data[5] as ChestFlagID, data[9] as MapMessageIndex)]
		},
		_ 		=> {
		    write!(notes, "\tUnknown opcode: {:02x}\n", data[0]).unwrap();
		    vec![]
		},
	    };

	    for i in 0..10 {
		if data[i] != 0 && (may_be_nonzero & (1 << i)) == 0 {
		    write!(notes, "\tByte #{i} unexpectedly set {:02x}\n", data[i]).unwrap();
		}
	    }

	    if notes.len() > 0 {
		unsafe {
		    // for debugging only
		    EVENT_OPS_INCOMPLETE |= 1 << data[0];
		}
		let mut s = String::new();
		write!(s, "Event [").unwrap();
		for i in 0..10 {
		    write!(s, " {:02x}", data[i]).unwrap();
		}
		write!(s, " ] warnings:").unwrap();
		warn!("{}\n{}", s, notes);
	    }

	    let raw : [u8; 10] = data[0..10].try_into().unwrap();

	    return Some(Event { raw, cond, program });
	}
    }
}


// ----------------------------------------
// NPCs

type NPCSpriteIndex = usize;
type NPCChatIndex = usize;

type MapTextIndex = usize;

pub enum NPCAction {
    PopupMessage(MapTextIndex),
    Chat(NPCChatIndex),
}

pub struct MapNPC {
    pub sprite : NPCSpriteIndex,
    pub talk_action : NPCAction,
    pub flags : u32,
}

// ----------------------------------------
// Map
pub struct Map {
    pub name : String,
    pub width : usize,
    pub height : usize,
    tiles : Vec<MapLayer<TileIndex>>,
    hotspots : MapLayer<EventIndex>,
    event_table : Vec<Event>,
    pub tileset : usize,
    pub song_nr : usize,
    pub flags : u32,
    pub first_person : bool, // Pseudo-3D view
    pub data : Vec<u8>,
}

const NUM_EVENT_TABLE_ENTRIES : usize = 254;
const EVENT_TABLE_ENTRIES_START : usize = 0x28;
const EVENT_TABLE_ENTRY_SIZE : usize = 0x0a;

pub fn new(map_nr : usize, src : &[u8]) -> Map {
    pinfo!("Map #{:x} {{", map_nr);
    assert!(src.len() > 0x28 + 10*255);

    assert!(src[0] == 0xff);
    assert!(src[1] == 0x00);
    assert!(src[2] == 0x00);

    let tileset = (src[3] - 1) as usize;
    let first_person : bool = match src[4] {
	0 => false,
	1 => true,
	_ => { perror!("  map#{:x} first-person flag is {:x}", map_nr, src[4]);
	       false},
    };
    let flags = src[5] as u32;
    // Current hypotheses / observations:
    // 0x01 = Tomb, Inn
    // 0x02 = Graveyard
    // 0x04 = ?
    // 0x08 = Tomb, Inn
    // 0x10 = ?
    // 0x20 = ?
    // 0x40 = Graveyard, Inn
    // 0x80 = Tomb of Marillon
    // flags 01, 02, 04 are mutually exclusive, exactly one is set at any time
    // flags 20, 40, 80 are mutually exclusive, exactly one is set at any time
    let song_nr = src[6] as usize;
    let width = src[7] as usize;
    let height = src[8] as usize;
    let name = amber_string::from_bytes(&src[0x09..0x27]).trim_end().to_string();
    let mut event_table = vec![];

    let mut event_types = vec![0;255];

    unsafe {
	// debugging: init traces for byte patterns: up to opcode 0x17, with 0x9 bytes each
	if EVENT_OPS_PARAMETERS.len() == 0 {
	    EVENT_OPS_PARAMETERS = BPInfer::new_2dvec(0x18, 0x9);
	}
    }

    {
	let mut found_empty = false;

	for i in 0..NUM_EVENT_TABLE_ENTRIES {
	    let pos = EVENT_TABLE_ENTRIES_START + i * EVENT_TABLE_ENTRY_SIZE;
	    let slice = &src[pos..(pos + EVENT_TABLE_ENTRY_SIZE)];

	    unsafe {
		for bi in 1..10 {
		    EVENT_OPS_PARAMETERS[slice[0] as usize][bi - 1].observe(slice[bi]);
		}
	    }

	    event_types[src[pos] as usize] += 1;
	    match Event::new(slice) {
		None    => { found_empty = true; },
		Some(a) => {
		    if found_empty {
			//warn!("\tEvent table index {i:02x}: should have been empty but is not\n");
		    };
		    event_table.push(a)
		}
	    }
	}
    }

    let layers_nr = if first_person { 2 } else { 3 };
    let last_event_end = EVENT_TABLE_ENTRIES_START + NUM_EVENT_TABLE_ENTRIES * EVENT_TABLE_ENTRY_SIZE;
    let layer_size = width * height;
    let total_layer_size = layer_size * layers_nr;
    let size = src.len();
    let map_layers_start = last_event_end + 177;
    info!("\tname               = {name}");
    info!("\tflags              = {flags:02x}");
    info!("\tmagic 10           = {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
	  src[map_layers_start - 10],
	  src[map_layers_start - 9],
	  src[map_layers_start - 8],
	  src[map_layers_start - 7],
	  src[map_layers_start - 6],
	  src[map_layers_start - 5],
	  src[map_layers_start - 4],
	  src[map_layers_start - 3],
	  src[map_layers_start - 2],
	  src[map_layers_start - 1]);
    info!("\tdim                = {width}x{height}");
    info!("\tlayers_nr          = {layers_nr}");
    info!("\tevent_end          = {last_event_end:#x} =\t{last_event_end}");
    info!("\tone_layer_size     = {layer_size:#x} =\t{layer_size}");
    info!("\tlayer_size         = {total_layer_size:#x} =\t{total_layer_size}");
    info!("\tlatest_layer_start = {:#x} =\t{}", size - total_layer_size, size - total_layer_size);
    if map_layers_start != size - total_layer_size {
	info!("\tnominal_start      = {:#x} =\t{}", map_layers_start, map_layers_start);
	info!("\tnominal_end        = {:#x} =\t{}", map_layers_start + total_layer_size, map_layers_start + total_layer_size);
	info!("\ttrailing_bytes     = {:#x} =\t{}",
	      size - total_layer_size - map_layers_start,
	      size - total_layer_size - map_layers_start);
	info!("\ttrailing_byte      = {:#x} =\t{}", src[total_layer_size + map_layers_start],
	      src[total_layer_size + map_layers_start]);
    }
    info!("\tsize               = {size:#x} =\t{size}");
    let npc_start = last_event_end;
    const NUM_NPCS : usize = 24;
    let npc_end = npc_start + NUM_NPCS * 7;
    info!("\tnpc_end            = {npc_end:#x} =\t{npc_end}");
    info!("\tevents             = {}", event_table.len());
    for i in 4..30 {
	info!("\tevents.{}           = {} {}", i, event_types[i], if event_types[i] > 0 {"nonzero"} else {""});
    }

    {
	let npc_personality_start = npc_start;
	let npc_sprite_start = npc_start + (NUM_NPCS * 2);
	let npc_flags_start = npc_start + (NUM_NPCS * 3);
	let npc_flags2_start = npc_start + (NUM_NPCS * 4);
	let npc_flags3_start = npc_start + (NUM_NPCS * 5);
	let npc_flags4_start = npc_start + (NUM_NPCS * 6);
	for n in 0..NUM_NPCS {
	    let personality = decode::u16(src, npc_personality_start + n*2);
	    let sprite = src[npc_sprite_start + n];
	    // decoding info may not be accurate for POV maps
	    let flags1 = src[npc_flags_start + n]; // 0 or 1
	    let flags2 = src[npc_flags2_start + n];
	    // 0x01: chase player and attack
	    // 0x02: stationary (coord pair only)
	    //       otherwise: 2x 0x120 coordinates
	    // 0x04: is used on POV maps only
	    // 0x10: text message instead of char reference
	    let flags3 = src[npc_flags3_start + n]; // 0 or 1
	    let flags4 = src[npc_flags4_start + n]; // 0 or 1

	    if personality != 0 {
		info!("\tNPC {:#02x} {} {:04x} sprite {:02x} {:02x} {:02x} {:02x} {:02x} {} {} {} {}", n, if (flags2 & 0x10) != 0 {"text"} else {"char"}, personality - 1, sprite, flags1, flags2, flags3, flags4,
		      if flags1 > 2 { "weird-flags1"  } else {""},
		      if (flags2 & 0x13) != flags2 { "weird-flags2"  } else {""},
		      if flags3 > 1 { "weird-flags3"  } else {""},
		      if flags4 > 1 { "weird-flags4"  } else {""},
		);
	    }
	}
    }

    let map = Map {
	name,
	width,
	height,
	tiles : vec![],
	hotspots : MapLayer { width :0, height:0, tiles:vec![] },
	event_table,
	tileset,
	song_nr,
	flags,
	first_person,
	data : src.to_vec(),
    };
    pinfo!("}}");
    return map;
}
