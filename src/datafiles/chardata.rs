// Copyright (C) 2023 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
use crate::datafiles::{decode, amber_string, map_string_table::MapStringTable, pixmap};
use enumset::{EnumSet, EnumSetType};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};


use std::assert;

use super::{item::{Item, KeyID}, pixmap::IndexedPixmap, string_fragment_table::StringFragmentTable};

// --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Stat {
    SkillAttack,
    SkillParry,
    SkillSwim,
    SkillListen,
    SkillFindTraps,
    SkillDisarmTraps,
    SkillPickLocks,
    SkillSearch,
    SkillReadMagicScrolls,
    SkillUseMagic,

    AttrStrength,
    AttrIntelligence,
    AttrDexterity,
    AttrSpeed,
    AttrConstitution,
    AttrCharisma,
    AttrLuck,
    AttrMagic,
    AttrAge,
}

const OFFSET_XP_STAGING : usize		= 0x00cc; // Will be added to XP_CURRENT later, used during interactions
const OFFSET_XP_CURRRENT : usize	= 0x00ce; // Current XP

#[derive(Copy, Clone, Debug)]
struct StatMetaInfo {
    name : &'static str,
    bytesized : bool, // offset_v has an u8 value, otherwise u16
    offset_v : usize,
    offset_max_v : usize,
    stat : Stat,
}

impl StatMetaInfo {
    pub const fn skill(name : &'static str, offset_v : usize, offset_max_v : usize, stat : Stat) -> StatMetaInfo {
	StatMetaInfo { name, offset_v, offset_max_v, stat, bytesized : true }
    }
    pub const fn attr(name : &'static str, offset_v : usize, offset_max_v : usize, stat : Stat) -> StatMetaInfo {
	StatMetaInfo { name, offset_v, offset_max_v, stat, bytesized : false }
    }
}

const STATS : [StatMetaInfo;10] = [
    StatMetaInfo::skill("ATK", 0x0006, 0x0010, Stat::SkillAttack),

    StatMetaInfo::attr("STR", 0x0048, 0x005c, Stat::AttrStrength),
    StatMetaInfo::attr("INT", 0x004a, 0x005e, Stat::AttrIntelligence),
    StatMetaInfo::attr("DEX", 0x004c, 0x0060, Stat::AttrDexterity),
    StatMetaInfo::attr("SPE", 0x004e, 0x0062, Stat::AttrSpeed),
    StatMetaInfo::attr("CON", 0x0050, 0x0064, Stat::AttrConstitution),
    StatMetaInfo::attr("CHA", 0x0052, 0x0066, Stat::AttrCharisma),
    StatMetaInfo::attr("LUC", 0x0054, 0x0068, Stat::AttrLuck),
    StatMetaInfo::attr("MAG", 0x0056, 0x006a, Stat::AttrMagic),
    StatMetaInfo::attr("AGE", 0x0058, 0x006c, Stat::AttrAge),
];


impl Stat {

    pub fn short_str(&self) -> &str {
	return match self {
	    Stat::SkillAttack		=> "ATK",
	    Stat::SkillParry		=> "PAR",
	    Stat::SkillSwim		=> "SWI",
	    Stat::SkillListen		=> "LIS",
	    Stat::SkillFindTraps	=> "F-T",
	    Stat::SkillDisarmTraps	=> "D-T",
	    Stat::SkillPickLocks	=> "P-L",
	    Stat::SkillSearch		=> "SEA",
	    Stat::SkillReadMagicScrolls	=> "RMS",
	    Stat::SkillUseMagic		=> "U-M",
	    Stat::AttrStrength		=> "STR",
	    Stat::AttrIntelligence	=> "INT",
	    Stat::AttrDexterity		=> "DEX",
	    Stat::AttrSpeed		=> "SPE",
	    Stat::AttrConstitution	=> "CON",
	    Stat::AttrCharisma		=> "CHA",
	    Stat::AttrLuck		=> "LUC",
	    Stat::AttrMagic		=> "MAG",
	    Stat::AttrAge		=> "AGE",
	}
    }
}

// --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct PointPool {
    pub current : usize,
    pub max : usize,
}

impl PointPool {
    pub fn new(current : usize, max : usize) -> Self {
	PointPool { current, max }
    }
}

// --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum InteractionTrigger {
    Ask(u16),
    Show(KeyID),
    Give(KeyID),
    Pay(usize),
    Feed(usize),
    AskJoin,
    Unknown(u8, u16),
}

impl InteractionTrigger {
    pub fn new(ty : u8, pattern : u16) -> Self {
	match ty {
	    InteractionTrigger::ASK	=> InteractionTrigger::Ask(pattern),
	    InteractionTrigger::SHOW	=> InteractionTrigger::Show(KeyID::new(pattern as usize)),
	    InteractionTrigger::GIVE	=> InteractionTrigger::Give(KeyID::new(pattern as usize)),
	    InteractionTrigger::PAY	=> InteractionTrigger::Pay(pattern as usize),
	    InteractionTrigger::FEED	=> InteractionTrigger::Feed(pattern as usize),
	    InteractionTrigger::JOIN	=> {if pattern > 0 { warn!("InteractionTrigerr::AskJoin with nonzero argument {pattern:x}");};
					    InteractionTrigger::AskJoin},
	    _				=> {warn!("Unknown InteractionTrigger({ty:x}, {pattern:x})");
					    InteractionTrigger::Unknown(ty, pattern)},
	}
    }

    const ASK : u8	= 0x01;
    const SHOW : u8	= 0x02;
    const GIVE : u8	= 0x03;
    const PAY : u8	= 0x04;
    const FEED : u8	= 0x05;
    const JOIN : u8	= 0x06;

    pub fn show(&self, fragment_table : &StringFragmentTable) -> String {
	match self {
	    InteractionTrigger::Ask(word)	=> format!("Ask({})", fragment_table.get(*word)),
	    InteractionTrigger::Show(key)	=> format!("Show({key})"),
	    InteractionTrigger::Give(key)	=> format!("Give({key})"),
	    InteractionTrigger::Pay(gp)		=> format!("Pay({gp} gp)"),
	    InteractionTrigger::Feed(food)	=> format!("Feed({food})"),
	    InteractionTrigger::AskJoin		=> format!("Ask:Join"),
	    InteractionTrigger::Unknown(t, pat)	=> format!("??(ty={t:x}, pat={pat:x})"),
	}
    }
}

// ----------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Reaction {
    Say(usize), // index into messages[]
    TeachWord(u16), // dictionary word
    GiveItem(usize), // index into items[]
    GiveGold(usize),
    GiveFood(usize),
    GiveXP(usize),
    CompleteQuest(usize), // Interaction flag, used for character
    ModifyStat(usize, usize, usize), // (amount, stat, modification type (1: increase, others:...?)
    Unknown(u8, u16, u8, u8),
}

impl Reaction {
    pub fn new(ty : u8, details : u16, data1 : u8, data2 : u8) -> Self {
	let mut warning = false;
	let (data1_0, data2_0, result) = match ty {
	    Reaction::SAY		=> (true, true, Reaction::Say(details as usize)),
	    Reaction::TEACH_WORD	=> (true, true, Reaction::TeachWord(details)),
	    Reaction::GIVE_ITEM		=> (true, true, Reaction::GiveItem(details as usize - 1)),
	    Reaction::GIVE_GOLD		=> (true, true, Reaction::GiveGold(details as usize)),
	    Reaction::GIVE_FOOD		=> (true, true, Reaction::GiveFood(details as usize)),
	    Reaction::MODIFY_STAT	=> (false, false, Reaction::ModifyStat(details as usize, data1 as usize, data2 as usize)),
	    Reaction::COMPLETE		=> (true, true, Reaction::CompleteQuest(details as usize)),
	    _		=> {
		warning = true;
		(false, false, Reaction::Unknown(ty, details, data1, data2))}
	};
	if data1_0 && data1 > 0 { warning = true; }
	if data2_0 && data2 > 0 { warning = true; }
	if warning {
	    warn!("  Unknown reaction: {:02x} {:04x} {:02x} {:02x}",
		  ty, details, data1, data2);
	}
	return result;
    }

    const SAY : u8		= 0x01;
    const TEACH_WORD : u8	= 0x02;
    const GIVE_ITEM : u8	= 0x03;
    const GIVE_GOLD : u8	= 0x04;
    const GIVE_FOOD : u8	= 0x05;
    const COMPLETE : u8		= 0x06;
    const MODIFY_STAT : u8	= 0x07;

    pub fn show(&self, fragment_table : &StringFragmentTable, messages : &Vec<String>) -> String {
	match self {
	    Reaction::Say(msg)			=> format!("Say(\"{}\")", if *msg >= messages.len() { "<invalid>".to_string() } else { messages[*msg].clone() }),
	    Reaction::TeachWord(word)		=> format!("TeachWord({})", fragment_table.get(*word)),
	    Reaction::GiveItem(item)		=> format!("GiveItem({item:x})"),
	    Reaction::GiveGold(gp)		=> format!("GiveGold({gp})"),
	    Reaction::GiveFood(amount)		=> format!("GiveFood({amount})"),
	    Reaction::GiveXP(num)		=> format!("GiveXP({num})"),
	    Reaction::ModifyStat(num, stat, t)	=> format!("ModifyStat({num}, {stat:04x}, {t})"),
	    Reaction::CompleteQuest(quest_id)	=> format!("CompleteQuest({quest_id:x})"),
	    Reaction::Unknown(ty, detail, a, b)	=> format!("??(ty={ty}, {detail:x}, {a:x}, {b:x})"),
	}
    }
}

// ----------------------------------------
#[derive(Debug, Clone)]
pub struct Interaction {
    pub id : usize,
    pub trigger : InteractionTrigger,
    pub reactions : Vec<Reaction>,
}

// ----------------------------------------

#[derive(EnumSetType, Debug)]
pub enum MagicSchool {
    White,
    Black,
    Grey,
    Special,
}

// --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct StatField {
    stat : Stat,
    current : usize,
    max : usize,
}

impl StatField {
    pub fn new(stat : Stat, current : usize, max : usize) -> Self {
	StatField { stat, current, max }
    }
}

#[derive(Clone)]
pub struct CharData {
    pub monster : bool,
    pub monster_gfx : usize,
    pub gender : usize,
    pub race : usize,
    pub class : usize,
    pub stats : Vec<StatField>,
    pub level : usize,
    pub magic_schools: EnumSet<MagicSchool>,
    pub used_hands : usize,
    pub used_fingers : usize,
    pub interaction_status_flag : usize, // number of flag to mark whether NPC is in "completed" state
    pub hp : PointPool, // "life points"
    pub sp : PointPool, // spell points
    pub gp : usize, // gold
    pub num_food : usize, // rations
    pub base_defense: usize,
    pub base_damage: usize,
    pub defense : usize,
    pub attack : usize,
    pub weight : usize,
    pub name : String,
    pub items : Vec<(usize, Item)>, // 0-8 are equipment slots
    // 0 neck
    // 1 head
    // 2 chest
    // 3 right hand
    // 4 body
    // 5 left hand
    // 6 right finger
    // 7 feet
    // 8 left finger
    pub portrait : Option<IndexedPixmap>,
    pub interactions : Vec<Vec<Interaction>>,  // not completed
    pub messages : Vec<String>,
}

impl CharData {
    const MONSTER_ICON_OFFSET : usize = 0x0a;

    // Index into the combat icon table
    pub fn combat_icon_nr(&self) -> usize {
	if self.monster {
	    self.monster_gfx - 1 + CharData::MONSTER_ICON_OFFSET
	} else {
	    self.class
	}
    }

    // index into the monster_gfx table
    pub fn combat_monster_gfx(&self) -> Option<usize> {
	if self.monster {
	    Some(self.monster_gfx - 1)
	} else {
	    None
	}
    }
}

fn print_unknown(data: &[u8], start: usize, len: usize) {
    let mut interesting = false;
    for n in start..start+len {
	if data[n] != 0 {
	    interesting = true;
	    break
	}
    }
    if !interesting {
	debug!("  unknown {:04x}: <zero>", start);
	return
    }
    let mut buf = format!("  unknown {:04x}:", start);
    for n in start..start+len {
	buf = buf + &format!(" {:02x}", data[n]);
    }
    debug!("{buf}");
}

impl CharData {
    pub fn new(fragment_table : &StringFragmentTable, npc_id : u16, data : &[u8]) -> Self {
	// unknown 0000-0001 (always 00 ff)
	let monster = data[0x0002] > 0;
	assert!(data[0x0002] < 2);
	let gender = data[0x0003] as usize;
	let race = data[0x0004] as usize;
	let class = data[0x0005] as usize;
	let stats = vec![
	    StatField::new(Stat::SkillAttack,		data[0x0006] as usize, data[0x0010] as usize),
	    StatField::new(Stat::SkillParry,		data[0x0007] as usize, data[0x0011] as usize),
	    StatField::new(Stat::SkillSwim,		data[0x0008] as usize, data[0x0012] as usize),
	    StatField::new(Stat::SkillListen,		data[0x0009] as usize, data[0x0013] as usize),
	    StatField::new(Stat::SkillFindTraps,	data[0x000a] as usize, data[0x0014] as usize),
	    StatField::new(Stat::SkillDisarmTraps,	data[0x000b] as usize, data[0x0015] as usize),
	    StatField::new(Stat::SkillPickLocks,	data[0x000c] as usize, data[0x0016] as usize),
	    StatField::new(Stat::SkillSearch,		data[0x000d] as usize, data[0x0017] as usize),
	    StatField::new(Stat::SkillReadMagicScrolls,	data[0x000e] as usize, data[0x0018] as usize),
	    StatField::new(Stat::SkillUseMagic,		data[0x000f] as usize, data[0x0019] as usize),

	    StatField::new(Stat::AttrStrength,		decode::u16(data, 0x0048) as usize, decode::u16(data, 0x005c) as usize),
	    StatField::new(Stat::AttrIntelligence,	decode::u16(data, 0x004a) as usize, decode::u16(data, 0x005e) as usize),
	    StatField::new(Stat::AttrDexterity,		decode::u16(data, 0x004c) as usize, decode::u16(data, 0x0060) as usize),
	    StatField::new(Stat::AttrSpeed,		decode::u16(data, 0x004e) as usize, decode::u16(data, 0x0062) as usize),
	    StatField::new(Stat::AttrConstitution,	decode::u16(data, 0x0050) as usize, decode::u16(data, 0x0064) as usize),
	    StatField::new(Stat::AttrCharisma,		decode::u16(data, 0x0052) as usize, decode::u16(data, 0x0066) as usize),
	    StatField::new(Stat::AttrLuck,		decode::u16(data, 0x0054) as usize, decode::u16(data, 0x0068) as usize),
	    StatField::new(Stat::AttrMagic,		decode::u16(data, 0x0056) as usize, decode::u16(data, 0x006a) as usize),
	    StatField::new(Stat::AttrAge,		decode::u16(data, 0x0058) as usize, decode::u16(data, 0x006c) as usize),
	];
	let mut magic_schools = EnumSet::new();
	if data[0x001a] & 0x02 > 0 { magic_schools |= MagicSchool::White };
	if data[0x001a] & 0x04 > 0 { magic_schools |= MagicSchool::Grey };
	if data[0x001a] & 0x08 > 0 { magic_schools |= MagicSchool::Black };
	if data[0x001a] & 0x80 > 0 { magic_schools |= MagicSchool::Special };
	assert!(data[0x001a] & !0x8e == 0);

	let level = data[0x001b] as usize;
	let used_hands = data[0x001c] as usize;
	let used_fingers = data[0x001d] as usize;
	let base_defense = data[0x001e] as usize;
	let base_damage = data[0x001f] as usize;
	//let _magic_bonus_weapon = data[0x0020] as usize;
	//let _magic_bonus_defense = data[0x0021] as usize;
	let inventory_counts = &data[0x0022..0x0022+(9 + 12)];
	let languages = data[0x0037];
	let current_language = data[0x0038]; // unused in game?
	// 0x39: always zero
	let physical_conditions = data[0x003a] as usize;
	let mental_conditions = data[0x003b] as usize;
	let join_chance = data[0x003c] as usize; // in percent: chance of joining party when asked
	let interaction_status_flag = data[0x003d] as usize;
	let monster_gfx = data[0x003e] as usize;
	if monster { assert!(monster_gfx > 0); }
	if !monster { assert!(monster_gfx == 0); }
	let spellcast_success_chance = data[0x3f];
	let minimum_magic_to_hit = data[0x40]; // Cannot be damaged if bonus is lower than this
	let morale_percentage = data[0x41]; // flee once this % of monsters of same type are defeated
	let battle_position = data[0x42]; // 0x01 / 0x02 : last row?
	let attacks_per_round = data[0x43];
	let monster_type = data[0x44]; // 01: undead, 02: demon, 04: immune to ailments
	let elemental_status = data[0x45]; // 01 fire, 02 earth, 04 water, 08 wind; lower nibble: immune, upper nibble: vulnerable (dbl damage)
	// unknown 003f-0047
	// unknown attribute at decode(data, 0x005a) / max decode(data, 0x006e)  (always zero)
	// unknown 0070-0085
	let hp = PointPool::new(decode::u16(data, 0x0086) as usize,
				decode::u16(data, 0x0088) as usize);
	let sp = PointPool::new(decode::u16(data, 0x008a) as usize,
				decode::u16(data, 0x008c) as usize);
	// unknown 008e-008f
	let gp = decode::u16(data, 0x0090) as usize;
	let num_food = decode::u16(data, 0x0092) as usize;
	let defense = decode::u16(data, 0x0094) as usize;
	let attack = decode::u16(data, 0x0096) as usize;
	// unknown 0098-00eb
	let weight = decode::u32(data, 0x00ec) as usize;
	let pre_name = amber_string::from_bytes(&data[0x0f0..0x100]);
	let name = match pre_name.find('\0') {
	    None    => pre_name,
	    Some(i) => pre_name[..i].to_string(),
	};
	// unknown 0x100-0x132

	debug!("----------------------------------------");
	debug!("NPC #{npc_id}/{npc_id:x}: {name}");
	debug!("  quest flag ID = {interaction_status_flag:x}");
	debug!("  size = {}/{:x}", data.len(), data.len());
	debug!("  monster_gfx = {monster_gfx}");
	debug!("  hp = {hp:?}");
	debug!("  sp = {sp:?}");
	debug!("  race = {race}");
	debug!("  class = {class}");
	print_unknown(data, 0x0020, 2);
	print_unknown(data, 0x0046, 2); // possible character classes?
	print_unknown(data, 0x0070, 22);
	print_unknown(data, 0x008e, 2); // often close to 1000
	print_unknown(data, 0x0098, 0x28);
	print_unknown(data, 0x0098 + 0x28, 0xeb - 0x98 - 0x28);
	print_unknown(data, 0x0100, 0x32);

	let mut items = vec![];
	let item_base_pos = 0x132;
	for item_nr in 0..(9+12) {
	    let item_pos = item_base_pos + item_nr * Item::BYTE_SIZE;
	    let count = inventory_counts[item_nr];
	    items.push((count as usize, Item::new(&fragment_table, &data[item_pos..(item_pos + Item::BYTE_SIZE)])));
	}

	let messages = if data.len() > 0x8d4 {
	    MapStringTable::new(&data[0x8d0..], fragment_table).strings
	} else { vec![] };

	for (slot, (count, item)) in items.iter().enumerate() {
	    if *count > 0 {
		debug!("  - item[{slot:x}] = {count} x {}", item.show_short());
	    }
	}

	const NUM_INTERACTIONS : usize = 20;
	let interaction_type_base = 0x47a;
	let interaction_pattern_base = interaction_type_base + NUM_INTERACTIONS;
	let interaction_reaction_base = interaction_pattern_base + NUM_INTERACTIONS * 2;
	let interaction_reaction_arg1_base = interaction_reaction_base + NUM_INTERACTIONS * 5;
	let interaction_reaction_arg2_base = interaction_reaction_arg1_base + NUM_INTERACTIONS * 5;
	let interaction_reaction_details_base = interaction_reaction_arg2_base + NUM_INTERACTIONS * 5;
	let mut interactions_all = vec![vec![], vec![]];

	for interaction_nr in 0..20 {
	    let ty = data[interaction_type_base + interaction_nr];
	    let pattern = decode::u16(data, interaction_pattern_base + interaction_nr * 2);
	    let reactions_ty = &data[(interaction_reaction_base + interaction_nr * 5)..];
	    let reactions_arg1 = &data[(interaction_reaction_arg1_base + interaction_nr * 5)..];
	    let reactions_arg2 = &data[(interaction_reaction_arg2_base + interaction_nr * 5)..];
	    let reactions_details = &data[(interaction_reaction_details_base + interaction_nr * 10)..];

	    if ty > 0 {
		let trigger = InteractionTrigger::new(ty, pattern);
		let mut reactions = vec![];
		for reaction_nr in 0..5 {
		    let reaction = reactions_ty[reaction_nr];
		    if reaction > 0 {
			reactions.push(Reaction::new(reaction, decode::u16(reactions_details, reaction_nr * 2),
						     reactions_arg1[reaction_nr], reactions_arg2[reaction_nr]));
		    }
		}
		//if interaction_nr < 10 {&interactions_all[0]} else {&interactions_all[1]}
		let interaction_type = if interaction_nr < 10 { 0 } else { 1 };
		interactions_all[interaction_type].push(Interaction { id : interaction_nr, trigger, reactions });
	    }
	};

	let image_base = 0x6aa;
	let portrait = if decode::u32(data, 0x6aa) > 0 {
	    Some(pixmap::new_icon_frame(&data[image_base..]))
	} else { None };
	debug!("  portrait = {}", portrait.is_some());
	for (interaction_type, interactions) in [("incomplete", &interactions_all[0]), ("complete", &interactions_all[1])] {
	    if interactions.len() > 0 {
		debug!("  interactions[{interaction_type}({interaction_status_flag:x})] = {}", interactions.len());
		for interaction in interactions {
		    let Interaction { id, trigger, reactions } = interaction;
		    debug!("    == #{id}:  {}", trigger.show(fragment_table));
		    for reaction in reactions {
			debug!("       - {}", reaction.show(fragment_table, &messages));
		    }
		}
	    }
	}

	return CharData {
	    monster,
	    monster_gfx,
	    gender,
	    race,
	    class,
	    stats,
	    interaction_status_flag,
	    used_hands,
	    used_fingers,
	    magic_schools,
	    level,
	    hp,
	    sp,
	    gp,
	    num_food,
	    base_defense,
	    base_damage,
	    defense,
	    attack,
	    weight,
	    name,
	    items,
	    portrait,
	    interactions : interactions_all,
	    messages,
	};
    }
}
