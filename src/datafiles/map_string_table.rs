use super::string_fragment_table::StringFragmentTable;
use crate::datafiles::decode;

pub struct MapStringTable {
    pub strings : Vec<String>
}

impl MapStringTable {
    pub fn new(bytes : &[u8], fragments : &StringFragmentTable) -> MapStringTable {
	let mut result = MapStringTable { strings : vec![] };
	let num_strings : usize = if bytes.len() < 2 { 0 } else { bytes[0] as usize };
	if num_strings > 0 {
	    let pos_table = &bytes[2..2*(num_strings+2)];
	    let body_table = &bytes[2*(num_strings+2)..];
	    for i in 0..num_strings {
		let u16_start = decode::u16(pos_table, i * 2);
		let u16_end = decode::u16(pos_table, (i + 1) * 2);
		let u16_size = u16_end - u16_start;
		let mut s : String = "".to_string();
		let str_indices = &body_table[(u16_start as usize * 2)..(u16_end as usize * 2)];
		for k in 0..u16_size {
		    let str_index = decode::u16(str_indices, k as usize * 2);
		    let token = &fragments.get(str_index);
		    if k > 0 && token.len() > 0 {
			let firstchar = token.chars().nth(0).unwrap();
			if (firstchar >= '0' && firstchar <= '9')
			    || (firstchar >= 'A' && firstchar <= 'Z')
			    || (firstchar == '-' || firstchar == '~') {
			    s.push(' ');
			}
		    }
		    s.push_str(token);
		}
		result.strings.push(s);
	    }
	}
	return result;
    }
}

