// Copyright (C) 2023 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};
#[allow(unused)]
use crate::{ptrace, pdebug, pinfo, pwarn, perror};

use std::{assert, fmt::Display};

use super::{decode, string_fragment_table::StringFragmentTable};

// --------------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct KeyID {
    pub id : usize,
}

impl KeyID {
    pub fn new(id : usize) -> Self {
	KeyID { id }
    }
}

impl Display for KeyID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	if self.id == 0 {
	    write!(f, "no-key")
	} else {
	    write!(f, "key#{:x}", self.id)
	}
    }
}

// --------------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct ItemType {
    ty : u8,
}


impl ItemType {
    pub fn new(ty : u8) -> Self {
	return ItemType{
	    ty
	}
    }

    const ARMOR : u8		= 0x00;
    const SHOES : u8		= 0x02;
    const SHIELD : u8		= 0x03;
    const MELEE_WEAPON : u8	= 0x04;
    const AMMUNITION : u8	= 0x06;
    const POTION : u8		= 0x09;
    const KEY : u8		= 0x0f;
    const ITEM : u8		= 0x10;
}

impl Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	match self.ty {
	    ItemType::ARMOR	=> write!(f, "armor"),
	    ItemType::SHOES	=> write!(f, "shoes"),
	    ItemType::SHIELD	=> write!(f, "shield"),
	    ItemType::MELEE_WEAPON=> write!(f, "melee-weapon"),
	    ItemType::AMMUNITION=> write!(f, "ammunition"),
	    ItemType::POTION	=> write!(f, "potion"),
	    ItemType::KEY	=> write!(f, "key"),
	    ItemType::ITEM	=> write!(f, "item"),
	    _			=> write!(f, "unknown({:x})", self.ty),
	}
    }
}

// --------------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
struct EquipSlot {
    slot : usize,
}

impl EquipSlot {
    pub fn new(slot : u8)  -> Self {
	return EquipSlot{
	    slot : slot as usize,
	}
    }

    const NONE : u8		= 0x00;
    const NECK : u8		= 0x01;
    const HEAD : u8		= 0x02;
    const CHEST : u8		= 0x03;
    const MAIN_HAND : u8	= 0x04;
    const ARMOR : u8		= 0x05;
    const OFF_HAND : u8		= 0x06;
    const RIGHT_FINGER : u8	= 0x07;
    const FEET : u8		= 0x08;
    const LEFT_FINGER : u8	= 0x09;
}

// --------------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct Item {
    icon : usize,
    item_type : ItemType,
    num_hands : usize,
    num_fingers : usize,
    allowed_classes : u32,
    bonus_shield_defense : isize,
    bonus_damage : isize,
    equip_slot : EquipSlot,
    buy_price : usize,
    weight : usize,
    key_id : KeyID,
    name : String,
    unknowns : [u8; 23],
}

impl Item {
    pub const BYTE_SIZE : usize = 0x28;

    pub fn new(fragment_table : &StringFragmentTable, data : &[u8]) -> Self {
	Item {
	    icon : data[0x00] as usize,
	    item_type : ItemType::new(data[0x01]),
	    num_hands : data[0x04] as usize,
	    num_fingers : data[0x05] as usize,
	    allowed_classes : decode::u16(data, 0x0e) as u32,
	    bonus_shield_defense : data[0x10] as isize,
	    bonus_damage : data[0x11] as isize,
	    equip_slot : EquipSlot::new(data[0x12]),
	    buy_price : decode::u16(data, 0x20) as usize,
	    weight : decode::u16(data, 0x22) as usize,
	    key_id : KeyID::new(decode::u16(data, 0x24) as usize),
	    name : fragment_table.get(decode::u16(data, 0x26)),
	    unknowns : [
		data[0x02],
		data[0x03],
		data[0x06],
		data[0x07],
		data[0x08],
		data[0x09],
		data[0x0a],
		data[0x0b],
		data[0x0c],
		data[0x0d],
		data[0x13],
		data[0x14],
		data[0x15],
		data[0x16],
		data[0x17],
		data[0x18],
		data[0x19],
		data[0x1a],
		data[0x1b],
		data[0x1c],
		data[0x1d],
		data[0x1e],
		data[0x1f],
	    ],
	}
    }

    pub fn show_short(&self) -> String {
	return format!("{} ({}) : {}",
		       self.name,
		       self.key_id,
		       self.item_type);
    }
}
