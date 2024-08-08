// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use crate::datafiles::{amber_string, decode, bytepattern::BPInfer};
use std::{fmt::Write, num::NonZeroU8};

use super::tile::TileFlags;

// ----------------------------------------
pub struct MapLayer<T> {
    pub width : usize,
    pub height : usize,
    pub tiles : Vec<T>,
}

impl<T : Clone + Copy> MapLayer<T> {
    fn new(width:usize, height: usize, data : &[u8], coerce : fn(u8) -> T) -> MapLayer<T> {
	MapLayer {
	    width, height,
	    tiles: data.iter().map(|x| coerce(*x)).collect()
	}
    }

    fn get(&self, x : usize, y : usize) -> T {
	assert!(x < self.width);
	assert!(y < self.height);
	return self.tiles[self.width * y + x];
    }
}

// ----------------------------------------

#[derive(Clone)]
#[derive(Copy)]
pub enum MapDir {
    NORTH,
    EAST,
    SOUTH,
    WEST
}

impl MapDir {
    pub fn rotate_right(self) -> MapDir {
	match self {
	    MapDir::NORTH	=> MapDir::EAST,
	    MapDir::EAST	=> MapDir::SOUTH,
	    MapDir::SOUTH	=> MapDir::WEST,
	    MapDir::WEST	=> MapDir::NORTH,
	}
    }

    pub fn rotate_left(self) -> MapDir {
	match self {
	    MapDir::NORTH	=> MapDir::WEST,
	    MapDir::EAST	=> MapDir::NORTH,
	    MapDir::SOUTH	=> MapDir::EAST,
	    MapDir::WEST	=> MapDir::SOUTH,
	}
    }

    pub fn xvec(self) -> isize {
	match self {
	    MapDir::EAST	=> 1,
	    MapDir::WEST	=> -1,
	    _			=> 0,
	}
    }

    pub fn yvec(self) -> isize {
	match self {
	    MapDir::SOUTH	=> 1,
	    MapDir::NORTH	=> -1,
	    _			=> 0,
	}
    }
}

// ----------------------------------------
// unsafe debugging code

struct DebugStats {
    event_ops_observed  : u32,
    /// flag for event ops that we failed to fully decode:
    event_ops_incomplete: u32,
    /// collect information about event ops:
    event_ops_parameters: Vec<Vec<BPInfer>>,
    /// Map header stats:
    header_bytes: Vec<BPInfer>,
    /// The four NPC flag bytes:
    npc_flags: Vec<BPInfer>,
    /// The four NPC flag bytes:
    labinfo_bytes: Vec<BPInfer>,
}

impl DebugStats {
    const EMPTY : DebugStats = DebugStats {
	event_ops_observed : 0,
	event_ops_incomplete : 0,
	event_ops_parameters : vec![],
	header_bytes : vec![],
	npc_flags : vec![],
	labinfo_bytes : vec![],
    };

    pub fn is_init(&self) -> bool {
	return self.event_ops_parameters.len() > 0;
    }
    pub fn init(&mut self) {
	self.event_ops_parameters = BPInfer::new_2dvec(0x20, 0x9);
	self.header_bytes = BPInfer::new_vec(0x9);
	self.npc_flags = BPInfer::new_vec(0x4);
	self.labinfo_bytes = BPInfer::new_vec(0x7);
    }
    pub fn print_summary(&self) {
	print!("* Map Header ({} bytes) ---\n", self.header_bytes.len());
	print!("    {}\n", BPInfer::explain_vec(&self.header_bytes));

	print!("* NPC flags ---\n");
	print!("    {}\n", BPInfer::explain_vec(&self.npc_flags));

	print!("* LabInfo bytes ---\n");
	print!("    {}\n", BPInfer::explain_vec(&self.labinfo_bytes));

	print!("* Event Ops ---\n");
	for op in 1..0x20 {
	    if 0 != self.event_ops_observed & 1 << op {
		let completed = if 0 == (self.event_ops_incomplete & 1 << op) {
		    "  complete"
		} else {
		    "INCOMPLETE"
		};
		print!("    {completed} event op type {op:02x} {}\n", BPInfer::explain_vec(&self.event_ops_parameters[op]));
	    }
	}
    }
}

static mut MAP2D_DEBUG_STATS : DebugStats = DebugStats::EMPTY;
static mut MAP3D_DEBUG_STATS : DebugStats = DebugStats::EMPTY;

unsafe fn debug_stats() -> &'static mut DebugStats {
    if DEBUG_STATS_2D {
	&mut MAP2D_DEBUG_STATS
    } else {
	&mut MAP3D_DEBUG_STATS
    }
}

static mut DEBUG_STATS_2D : bool = false;

// static mut EVENT_OPS_INCOMPLETE : u32 = 0x00; /// flag for event ops that we failed to fully decode
// static mut EVENT_OPS_PARAMETERS : Vec<Vec<BPInfer>> = vec![]; /// collect information about event ops

unsafe fn event_debug_summary() {
    print!("== 2D maps ================================================================================\n");
    MAP2D_DEBUG_STATS.print_summary();
    print!("== 3D maps ================================================================================\n");
    MAP3D_DEBUG_STATS.print_summary();
    print!("===========================================================================================\n");
}

pub fn debug_summary() {
    unsafe {
	event_debug_summary();
    }
}

// ----------------------------------------
// Map layer elements
type TileIndex = Option<NonZeroU8>;
type EventIndex = Option<NonZeroU8>;

// ----------------------------------------
// Event is a DSL for describing actions
// attached to map events

type Keyword = usize;
type ImageIndex = usize;
type MapMessageIndex = usize; // Map string index
type MapIndex = usize;
type ChestIndex = usize; // number of CHESTDATA.AMB entry
type ChestFlagID = usize; // flag to store whether chest has been emptied

#[derive(Clone)]
pub enum EventOp {
    LockedDoor(usize), // lock pick difficulty
    PopupMessage(Option<ImageIndex>, MapMessageIndex),
    LearnKeyword(Keyword),
    Teleport(usize, usize, Option<MapIndex>), // MapIndex is None if teleport is on same map
    ChestAccess(ChestIndex, ChestFlagID, MapMessageIndex),
    RestoreLP, // restore all life points
    RestoreSP, // restore all spell points
    WinGame,       // win game
}

#[derive(Clone)]
pub enum EventCondition {
    Enter,
    Look
}

#[derive(Clone)]
pub struct Event {
    pub raw: [u8;10],
    _cond : EventCondition,
    _program : Vec<EventOp>,
}

impl Event {
    const EMPTY : Event = Event {
	raw : [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
	_cond : EventCondition::Enter,
	_program : vec![],
    };
    fn new(data : &[u8]) -> Option<Event> {
	const OP_TELEPORT	: u8 = 0x01;
	const OP_LOCKED_DOOR	: u8 = 0x02;
	const OP_MESSAGE	: u8 = 0x03;
	const OP_CHEST		: u8 = 0x04;
	//const OP_6		: u8 = 0x06; // ?? encounter
	//const OP_8		: u8 = 0x08; // ?? continuous message
	const OP_RESTORE_LP	: u8 = 0x0b; // data[4]: message
	const OP_RESTORE_SP	: u8 = 0x0c; // data[4]: message
	//const OP_3D_STORE	: u8 = 0x12; // ?? Only used in first-person view, probably stores/guilds
	//const OP_3D_BARRIER     : u8 = 0x13; // ?? need crowbar to get through?
	//const OP_3D_LOCKED_DOOR	: u8 = 0x14; // ?? Only used in first-person view
        // param 04: other event to trigger (if unlocked with special key?)
	// e.g., door to Family Home:   14 01 00 00 1c 04 00 97 00 00
	const OP_WIN_GAME	: u8 = 0x17; // Last OP

	if data[0] == 1 && data[1] == 0 && data[2] == 0 && data[3] == 0 && data[7] == 0 {
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

	    unsafe {
		debug_stats().event_ops_observed |= 1 << data[0];
	    }
	    if notes.len() > 0 {
		unsafe {
		    // for debugging only
		    debug_stats().event_ops_incomplete |= 1 << data[0];
		}
		let mut s = String::new();
		write!(s, "Event [").unwrap();
		for i in 0..10 {
		    write!(s, " {:02x}", data[i]).unwrap();
		}
		write!(s, " ] warnings:").unwrap();
		warn!("{}\n{}", s, notes);
	    } else {
		let mut s = String::new();
		write!(s, "Event [").unwrap();
		for i in 0..10 {
		    write!(s, " {:02x}", data[i]).unwrap();
		}
		write!(s, " ] OK").unwrap();
		info!("{}\n{}", s, notes);
	    }

	    let raw : [u8; 10] = data[0..10].try_into().unwrap();

	    return Some(Event { raw, _cond: cond, _program: program });
	}
    }
}

// ----------------------------------------
// 3D Labblock references

#[derive(Clone)]
#[derive(Copy)]
pub struct LabRef {
    pub flags : TileFlags,
    pub fg_image : usize,
    pub bg_image : usize,
    pub magic : u8,
}

// ----------------------------------------
// NPCs

type NPCSpriteIndex = usize;
type NPCIndex = usize;

#[derive(Clone)]
pub enum NPCAction {
    PopupMessage(MapMessageIndex),
    Chat(NPCIndex),
}

#[derive(Clone)]
pub enum NPCMovement {
    Attack,                     // move towards player and trigger fight when on same tile
    Stationary,
    Cycle(Vec<Option<(usize, usize)>>), // cycle through coordinates.  None means that the NPC is "out"
}

#[derive(Clone)]
pub struct MapNPC {
    pub sprite : NPCSpriteIndex,
    pub talk_action : NPCAction,
    pub flags : u32,
    pub start_pos : (usize, usize),
    pub movement : NPCMovement,
}

impl MapNPC {
    const NUM : usize = 24;
    const SIZE : usize = 7;
    const MOVEMENT_CYCLE_LEN : usize = 0x120;

    const FLAG_CHAT_TEXTMESSAGE	: u32 = 0x00001000; // only show text popup.  If not set, trigger full NPC chat
    const FLAG_STATIONARY	: u32 = 0x00000200;
    const FLAG_CHASE_AND_ATTACK	: u32 = 0x00000100;

    pub fn lab_ref(&self, map : &Map) -> LabRef {
	return map.lab_info[self.sprite - 1];
    }

    pub fn hostile(&self) -> bool {
	return 0 != self.flags & MapNPC::FLAG_CHASE_AND_ATTACK;
    }

    pub fn decode_all(npc_decl : &[u8], npc_movement : &[u8]) -> Vec<MapNPC> {
	let mut npc_movement_offset = 0;
	let mut npcs = vec![];

	let npc_sprite_start = MapNPC::NUM * 2;
	let npc_flags_start = MapNPC::NUM * 3;
	let npc_flags2_start = MapNPC::NUM * 4;
	let npc_flags3_start = MapNPC::NUM * 5;
	let npc_flags4_start = MapNPC::NUM * 6;
	for n in 0..MapNPC::NUM {
	    let personality = decode::u16(npc_decl, n*2);
	    let sprite = npc_decl[npc_sprite_start + n] as NPCSpriteIndex;
	    let flags1 = npc_decl[npc_flags_start + n] as u32; // 0 or 1
	    let flags2 = npc_decl[npc_flags2_start + n] as u32;
	    // 0x01: chase player and attack
	    // 0x02: stationary (coord pair only)
	    //       otherwise: 2x 0x120 coordinates
	    // 0x04: is used on POV maps only
	    // 0x10: text message instead of char reference
	    let flags3 = npc_decl[npc_flags3_start + n] as u32; // 0 or 1
	    let flags4 = npc_decl[npc_flags4_start + n] as u32; // 0 or 1

	    let flags = flags1 | (flags2 << 8) | (flags3 << 16) | (flags4 << 24);

	    if personality != 0 {
		let talk_action = if 0 != flags & MapNPC::FLAG_CHAT_TEXTMESSAGE {
		    NPCAction::PopupMessage(personality as MapMessageIndex)
		} else {
		    NPCAction::Chat(personality as NPCIndex)
		};

		let (movement, num_positions) = if 0 != flags & MapNPC::FLAG_STATIONARY {
		    (NPCMovement::Stationary, 1)
		} else if 0 != flags & MapNPC::FLAG_CHASE_AND_ATTACK {
		    (NPCMovement::Attack, 1)
		} else {
		    let cycle_len = MapNPC::MOVEMENT_CYCLE_LEN;
		    let x_coords = npc_movement_offset;
		    let y_coords = npc_movement_offset + cycle_len;
		    let coords : Vec<Option<(usize, usize)>> = npc_movement[x_coords..x_coords+cycle_len].iter()
			.zip(&npc_movement[y_coords..y_coords+cycle_len])
			.map(|(x, y)| if *x <= 6 || *y <= 6 { None } else { Some(((*x - 1) as usize, (*y - 1) as usize)) }).collect();

		    // for i in 0..MapNPC::MOVEMENT_CYCLE_LEN {
		    // 	let (cx, cy) = coords[i];
		    // 	let (nx, ny) = if i == MapNPC::MOVEMENT_CYCLE_LEN - 1 { coords[0] } else { coords[i+1] };
		    // 	if isize::abs(cx as isize - nx as isize) > 1
		    // 	    || isize::abs(cy as isize - ny as isize) > 1 {
		    // 		warn!("Non-contiguous map movement for NPC {n:x}, i={i}: {:?}->{:?}, offsets {:x} and {:x}",
		    // 		      (cx, cy), (nx, ny),
		    // 		      x_coords + i,
		    // 		      y_coords + i,
		    // 		);
		    // 		break;
		    // 	    }
		    // }

		    (NPCMovement::Cycle(coords), cycle_len)
		};

		debug!("\tNPC {n:x}: movement_offset = {npc_movement_offset:x}, num_positions={num_positions:x}");

		// Steps are stored as sequence of x coords followed by sequence of y coords
		let start_pos = ((npc_movement[npc_movement_offset] - 1) as usize,
				 (npc_movement[npc_movement_offset + num_positions] - 1) as usize);
		npc_movement_offset += 2 * num_positions;


		info!("\tNPC {:#02x} {} {:04x} sprite {:02x} {:02x} {:02x} {:02x} {:02x} {} {} {} {}", n, if (flags2 & 0x10) != 0 {"text"} else {"char"}, personality - 1, sprite, flags1, flags2, flags3, flags4,
		      if flags1 > 2 { "weird-flags1"  } else {""},
		      if (flags2 & 0x13) != flags2 { "weird-flags2"  } else {""},
		      if flags3 > 1 { "weird-flags3"  } else {""},
		      if flags4 > 1 { "weird-flags4"  } else {""},
		);

		unsafe {
		    debug_stats().npc_flags[0].observe(flags1 as u8);
		    debug_stats().npc_flags[1].observe(flags2 as u8);
		    debug_stats().npc_flags[2].observe(flags3 as u8);
		    debug_stats().npc_flags[3].observe(flags4 as u8);
		}

		npcs.push(MapNPC {
		    sprite,
		    talk_action,
		    flags,
		    start_pos,
		    movement,
		});
	    }
	}
	return npcs;
    }
}

// ----------------------------------------
// Map

/// Light status of the map
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Illumination {
    /// Always bright
    Always,
    /// Light depends on time of day
    Daylight,
    /// Players must bring their own light
    Never,
}

/// What kind of environment the map is representing
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Environment {
    Wilderness,
    City,
    Dungeon,
}

pub struct Map {
    pub name : String,
    pub width : usize,
    pub height : usize,
    pub num_layers : usize,
    pub tiles : Vec<MapLayer<TileIndex>>,
    hotspots : MapLayer<EventIndex>,
    pub event_table : Vec<Event>,
    pub tileset : usize,
    pub song_nr : usize,
    pub lab_info : Vec<LabRef>,
    pub flags: u32,
    /// Resting allowed
    pub can_rest : bool,
    /// Mapshow spell allowed
    pub can_mapshow : bool,
    pub illumination: Illumination,
    pub environment: Environment,
    pub first_person : bool, // Pseudo-3D view
    pub npcs : Vec<MapNPC>,
    pub data : Vec<u8>,
}

impl Map {
    pub fn tile_at(&self, layer : usize, x : usize, y : usize) -> Option<usize> {
	if layer >= self.tiles.len() {
	    return None;
	}
	let tm = &self.tiles[layer];
	if x >= tm.width || y >= tm.height {
	    return None;
	}
	return tm.get(x, y).map(|x| x.get() as usize);
    }



    // Constructs tuples of entities to draw
    // (labref, distance, -1 / 0 / 1)
    pub fn lab_view(&self, x : isize, y : isize, dir : MapDir) -> Vec<(LabRef, usize, isize)> {
	let mut results = vec![];
	let right_dir = dir.rotate_right();

	for dist in [3, 2, 1, 0] {
	    for leftright in [-1, 1, 0] {
		let rx = x + (dir.xvec() * dist) + (right_dir.xvec() * leftright);
		let ry = y + (dir.yvec() * dist) + (right_dir.yvec() * leftright);
		if rx >= 0 && ry >= 0 {
		    match self.tile_at(0, rx as usize, ry as usize) {
			Some(t) => {
			    if t > 0 && t <= self.lab_info.len() {
				results.push((self.lab_info[t - 1], dist as usize, leftright));
			    }
			},
			None => {},
		    }
		}
	    }
	}
	return results;
    }

    pub fn hotspot_at(&self, x : usize, y : usize) -> Option<usize> {
	let tm = &self.hotspots;
	if x >= tm.width || y >= tm.height {
	    return None;
	}
	return tm.get(x, y).map(|x| x.get() as usize);
    }
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
    let illumination = match flags & 0x07 {
	0x01 => Illumination::Always,
	0x02 => Illumination::Daylight,
	0x04 => Illumination::Never,
	_    => { perror!("  map#{:x} illumination flags are odd {:x}", map_nr, flags & 0x07);
	          Illumination::Always }
    };
    let environment = match flags & 0xe0 {
	0x20 => Environment::Wilderness,
	0x40 => Environment::City,
	0x80 => Environment::Dungeon,
	_    => { perror!("  map#{:x} environment flags are odd {:x}", map_nr, flags & 0xe0);
	          Environment::Wilderness }
    };
    let can_mapshow = flags & 0x08 > 0;
    let can_rest = flags & 0x10 > 0;

    let song_nr = src[6] as usize;
    let width = src[7] as usize;
    let height = src[8] as usize;
    let name = amber_string::from_bytes(&src[0x09..0x27]).trim_end().to_string();
    let mut event_table = vec![];

    let mut event_types = vec![0;255];

    unsafe {
	// debugging: init traces for byte patterns: up to opcode 0x17, with 0x9 bytes each
	if !MAP2D_DEBUG_STATS.is_init() {
	    MAP2D_DEBUG_STATS.init();
	}
	if !MAP3D_DEBUG_STATS.is_init() {
	    MAP3D_DEBUG_STATS.init();
	}
	DEBUG_STATS_2D = !first_person;
	BPInfer::observe_vec(&mut debug_stats().header_bytes, &src);
    }

    // Decode events
    {
	for i in 0..NUM_EVENT_TABLE_ENTRIES {
	    let pos = EVENT_TABLE_ENTRIES_START + i * EVENT_TABLE_ENTRY_SIZE;
	    let slice = &src[pos..(pos + EVENT_TABLE_ENTRY_SIZE)];

	    unsafe {
		BPInfer::observe_vec(&mut debug_stats().event_ops_parameters[slice[0] as usize],
				     &slice[1..]);
	    }

	    event_types[src[pos] as usize] += 1;
	    match Event::new(slice) {
		None    => { },
		Some(a) => {
		    // just in case there are some gaps, fill with empty events
		    while event_table.len() < i {
			event_table.push(Event::EMPTY);
		    }
		    event_table.push(a)
		}
	    }
	}
    }

    // Prepare map layer sizes, since NPC data is stored before and after the map layers
    let num_layers = if first_person { 2 } else { 3 };
    let last_event_end = EVENT_TABLE_ENTRIES_START + NUM_EVENT_TABLE_ENTRIES * EVENT_TABLE_ENTRY_SIZE;
    let layer_size = width * height;
    let total_layer_size = layer_size * num_layers;

    // NPCs section (we decode later, once we know where NPC movement starts)
    let npc_start = last_event_end;
    let npc_end = npc_start + MapNPC::NUM * MapNPC::SIZE;

    // Check magic bytes before 3D data section
    if src[npc_end..9+npc_end] != [0x01, 0x20, 0x0c, 0x1e, 0x18, 0x3c, 0x05, 0x0c, 0x0c] {
	warn!("map #{:02x}: End of NPC section has wrong magic bytes: {:x?} ", map_nr, &src[npc_end..9+npc_end]);
    }

    let mut lab_info = vec![]; // only for 3D maps
    let mut map_layers_start = npc_end + 9; // skip over magic 9 bytes

    if num_layers == 2 {
	// 3D data section: check for two-layer maps (3D maps), otherwise assume size zero
	let num_labblock_entries = if num_layers == 2 { src[npc_end + 9] as usize } else { 0 };
	let labblock_start = npc_end + 10; // skip over magic 9 bytes + size
	const LABBLOCK_SIZE : usize = 7;
	let labblock_end = labblock_start + num_labblock_entries * LABBLOCK_SIZE;
	let labblock_section = &src[labblock_start..labblock_end];

	for labblock_index in 0..num_labblock_entries {
	    let labblock_head_pos = labblock_index * 4;
	    let labblock_2_pos = num_labblock_entries * 4 + labblock_index;
	    let labblock_3_pos = num_labblock_entries * 5 + labblock_index;
	    let labblock_4_pos = num_labblock_entries * 6 + labblock_index;

	    let labblock_flags = TileFlags::new(&labblock_section[labblock_head_pos..4+labblock_head_pos]);
	    let fg_image = labblock_section[labblock_2_pos] as usize;
	    let bg_image = labblock_section[labblock_3_pos] as usize;
	    let magic = labblock_section[labblock_4_pos];
	    let labinfo = LabRef {
		flags : labblock_flags,
		fg_image,
		bg_image,
		magic,
	    };
	    // unsafe {
	    // 	for i in 0..4 {
	    // 	    debug_stats().labinfo_bytes[i].observe(labblock_head[i]);
	    // 	}
	    // 	debug_stats().labinfo_bytes[4].observe(labblock_2);
	    // 	debug_stats().labinfo_bytes[5].observe(labblock_3);
	    // 	debug_stats().labinfo_bytes[6].observe(labblock_4);
	    // }
	    lab_info.push(labinfo);
	    info!("\tlabblock.{labblock_index} = {:08x} {fg_image:02x} {bg_image:02x} {magic:02x}",
		  labblock_flags.flags);
	};
	// map layers start after the lab block section, if present
	info!("\tlabblock_start     = {labblock_start:#x} =\t{labblock_start}");
	info!("\tlabblock_end       = {labblock_end:#x} =\t{labblock_end}");
	map_layers_start = labblock_end;
    }

    let size = src.len();

    let npcs = MapNPC::decode_all(&src[npc_start..npc_end],
				  &src[map_layers_start + total_layer_size..]);


    info!("\tname               = {name}");
    info!("\tflags              = {flags:#02x}");
    info!("\ttileset            = {tileset:#02x}");
    info!("\ttotal_size         = {size:#x} =\t{size}");
    info!("\tmagic 9            = {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
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
    info!("\tnum_layers         = {num_layers}");
    info!("\tevent_end          = {last_event_end:#x} =\t{last_event_end}");
    info!("\tone_layer_size     = {layer_size:#x} =\t{layer_size}");
    info!("\ttotal_layer_size   = {total_layer_size:#x} =\t{total_layer_size}");
    info!("\tlatest_layer_start = {:#x} =\t{}", size - total_layer_size, size - total_layer_size);
    if map_layers_start != size - total_layer_size {
	info!("\tlayer[0]_start     = {:#x} =\t{}", map_layers_start, map_layers_start);
	info!("\tlayer[1].start     = {:#x} =\t{}", map_layers_start + layer_size, map_layers_start + layer_size);
	if num_layers > 2 {
	    info!("\tlayer[2].start     = {:#x} =\t{}", map_layers_start + layer_size*2, map_layers_start + layer_size*2);
	}
	info!("\tlayer[-1].end      = {:#x} =\t{}", map_layers_start + total_layer_size, map_layers_start + total_layer_size);
	info!("\ttrailing_bytes     = {:#x} =\t{}",
	      size - total_layer_size - map_layers_start,
	      size - total_layer_size - map_layers_start);
	info!("\ttrailing_byte      = {:#x} =\t{}", src[total_layer_size + map_layers_start],
	      src[total_layer_size + map_layers_start]);
    }
    info!("\tsize               = {size:#x} =\t{size}");
    info!("\tnpc_end            = {npc_end:#x} =\t{npc_end}");
    info!("\tevents             = {}", event_table.len());
    for i in 4..30 {
	info!("\tevents.{}           = {} {}", i, event_types[i], if event_types[i] > 0 {"nonzero"} else {""});
    }

    let map2 = MapLayer::new(width, height, &src[layer_size+map_layers_start..2*layer_size+map_layers_start], NonZeroU8::new);
    let mut tiles = vec![MapLayer::new(width, height, &src[map_layers_start..layer_size+map_layers_start], NonZeroU8::new)];
    let hotspots;
    if num_layers == 2 {
	hotspots = map2;
    } else {
	tiles.push(map2);
	hotspots = MapLayer::new(width, height, &src[2*layer_size+map_layers_start..3*layer_size+map_layers_start], NonZeroU8::new);
    }

    let map = Map {
	name,
	width,
	height,
	num_layers,
	tiles,
	hotspots,
	event_table,
	lab_info,
	tileset,
	song_nr,
	flags,
	first_person,
	npcs,
        can_rest,
        can_mapshow,
        illumination,
        environment,
	data : src.to_vec(),
    };
    pinfo!("}}");
    return map;
}
