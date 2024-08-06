// Copyright (C) 2024 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

// Handling for decompressed AMBERDEV.UDO resources

use std::ops::Deref;

use super::{string_fragment_table::StringFragmentTable, amber_string, decode};

/// Game file language
#[derive(Debug, Clone, Copy)]
enum Language {
    DE, EN
}

/// Positions of special entities within an Amberdev file
pub struct Positions {
    /// start of the string fragment table
    pub string_fragment_table: usize,
    /// The string "CODETXT.AMB", relative to which a number of valuable pieces of data are stored
    pub codetxt_amb: usize,
    /// Start of the table of spell names
    pub spell_name_table: usize,
    /// Table of merchant names
    pub merchant_name_table: usize,
    /// day, night, and dusk BG colour palettes
    pub daylight_tables: usize,
}

impl Positions {
    fn new() -> Self {
	Self {
	    string_fragment_table: 0,
	    codetxt_amb: 0,
	    spell_name_table: 0,
	    merchant_name_table: 0,
	    daylight_tables: 0,
	}
    }
}

pub struct Amberdev {
    pub data: Vec<u8>,
    pub string_fragments: StringFragmentTable,
    pub language: Language,
    pub positions: Positions,
    pub spell_names: Vec<Vec<String>>,
    pub merchant_names: Vec<String>,
    pub song_names: Vec<String>,
}

impl Amberdev {
    const COMBAT_PALETTE_OFFSET: usize = 0x5e5; // relative to the string "CODETXT.AMB"
    const COMBAT_PALETTE_LENGTH: usize = 0x20;
    const COMBAT_PALETTE_SPECIALISATION_LENGTH: usize = 14 * 6;

    pub fn new(data: Vec<u8>) -> Self {
	let data_len = data.len();
	let mut amberdev = Self {
	    data,
	    string_fragments: StringFragmentTable::new(&vec![]),
	    language: Language::EN,
	    positions: Positions::new(),
	    spell_names: vec![],
	    merchant_names: vec![],
	    song_names: vec![],
	};
	let (language, string_fragment_table) = match amberdev.find_string_fragment_table() {
	    None => panic!("Could not find string fragment table in decompressed AMBERDEV.UDO ({} bytes)", data_len),
	    Some(x) => x,
	};
	amberdev.language = language;
	amberdev.positions.string_fragment_table = string_fragment_table;
	amberdev.string_fragments = StringFragmentTable::new(&amberdev[string_fragment_table..]);
	amberdev.positions.codetxt_amb = amberdev.find_string_anywhere(0x31000, "CODETXT.AMB").expect("No reference to CODETXT.AMB in AMBERDEV.UDO");
	amberdev.positions.spell_name_table = 6 + amberdev.find_bytes_anywhere(0x4db00, &[0, 0, 0, 0, 0x01, 0x62]).expect("No spell name table foundin AMBERDEV.UDO");
	amberdev.positions.merchant_name_table = amberdev.positions.spell_name_table + 0x0004b230 - 0x0004ac00 - 2;
	amberdev.positions.daylight_tables = amberdev.positions.spell_name_table + 0x000473a4 - 0x0004ac0a;
	amberdev.song_names = amber_string::vec_from_terminated_bytes(&amberdev[amberdev.positions.codetxt_amb + 0x151..]);

	for (i, n) in amberdev.song_names.iter().enumerate() {
	    println!(" SONG {i}: {n}");
	}

	amberdev.spell_names = amberdev.extract_spell_names();
	amberdev.merchant_names = amberdev.extract_merchant_names();

	return amberdev;
    }

    pub fn combat_palette(&self) -> &[u8] {
	let offset = self.positions.codetxt_amb + Amberdev::COMBAT_PALETTE_OFFSET;
	return &self[offset..offset+Amberdev::COMBAT_PALETTE_LENGTH];
    }

    pub fn combat_palette_specialisation_table(&self) -> &[u8] {
	let offset = self.positions.codetxt_amb + Amberdev::COMBAT_PALETTE_OFFSET + Amberdev::COMBAT_PALETTE_LENGTH;
	return &self[offset..offset+Amberdev::COMBAT_PALETTE_SPECIALISATION_LENGTH];
    }

    pub fn len(&self) -> usize {
	return self.data.len();
    }

    fn extract_merchant_names(&self) -> Vec<String> {
	let mut pos = self.positions.merchant_name_table;
	let mut result = vec![];
	let len = 30;
	while pos < self.len() {
	    if self[pos] < 0x20 {
		break;
	    }
	    result.push(amber_string::from_bytes(&self[pos..pos+len]).trim_end().to_string());
	    pos += len;
	}
	return result;
    }

    fn extract_spell_names(&self) -> Vec<Vec<String>> {
	let start = self.positions.spell_name_table;
	let mut result = vec![];
	for school_nr in 0..7 {
	    let mut pos  = start + 30 * 2 * school_nr;
	    let mut current = vec![];
	    while pos < self.len() {
		let entry = decode::u16(&self, pos);
		if entry == 0 {
		    break;
		}
		current.push(self.string_fragments.get(entry));
		pos += 2;
	    }
	    result.push(current);
	}
	return result;
    }

    fn find_string_fragment_table(&self) -> Option<(Language, usize)> {
	let keywords = [ ("MENSCH", Language::DE),
			   ("HUMAN", Language::EN), ];
	let search_starts = [
	    0x21700, // likely to hit
	    0x0,     // worst case: search in entire file
	];

	for search_start in search_starts {
	    for (keyword, language) in keywords.iter() {
		match self.find_string(search_start, keyword) {
		    Some(offset) => { return Some((*language, offset - 1)); },
		    None => {},
		}
	    }
	}
	return None;
    }

    /// Find offset for the given `needle`, at or after `start`
    pub fn find_bytes(&self, start: usize, needle: &[u8]) -> Option<usize> {
	let mut pos = start;
	for w in self.data[start..].windows(needle.len()) {
	    if w == needle {
		return Some(pos);
	    }
	    pos += 1;
	}
	return None;
    }

    /// Find offset for the given `needle` anywhere, but try searching from `start` first
    pub fn find_bytes_anywhere(&self, heuristic_start: usize, needle: &[u8]) -> Option<usize> {
	let result = self.find_bytes(heuristic_start, needle);
	if result.is_some() {
	    return result;
	}
	return self.find_bytes(0, needle);
    }

    pub fn find_string(&self, start: usize, needle: &str) -> Option<usize> {
	self.find_bytes(start, &amber_string::to_bytes(needle))
    }

    pub fn find_string_anywhere(&self, heuristic_start: usize, needle: &str) -> Option<usize> {
	self.find_bytes_anywhere(heuristic_start, &amber_string::to_bytes(needle))
    }

}

impl AsRef<[u8]> for Amberdev {
    fn as_ref(&self) -> &[u8] {
	return &self.data;
    }
}

impl Deref for Amberdev {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
	return &self.data;
    }
}
