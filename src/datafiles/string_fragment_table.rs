// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use super::amber_string;

pub struct StringFragmentTable {
    fragments : Vec<String>
}

impl StringFragmentTable {
    pub fn new(bytes : &[u8]) -> StringFragmentTable {
	let mut result = StringFragmentTable { fragments : vec!["<?-NUL-?>".to_string()] };
	let mut offset = 0;

	while offset < bytes.len() {
	    let str_len = bytes[offset] as usize;
	    if str_len == 0 {
		/* End of string table */
		break;
	    }
	    let str_vec = &bytes[offset+1..(offset+str_len)];
	    let str : String = amber_string::from_bytes(&str_vec);
	    if str == "#" {
		result.fragments.push("\n".to_string());
	    } else {
		result.fragments.push(str);
	    }
	    offset += str_len;
	};
	return result;
    }

    pub fn len(&self) -> usize {
	return self.fragments.len();
    }

    pub fn get(&self, index : u16) -> String {
	if index as usize >= self.fragments.len() {
	    error!("Invalid string index {index}, max is {}", self.fragments.len() - 1);
	    return "<?-?>".to_string();
	}
	return self.fragments[index as usize].clone();
    }

    pub fn get_str(&self, index : u16) -> &str {
	if index as usize >= self.fragments.len() {
	    error!("Invalid string index {index}, max is {}", self.fragments.len() - 1);
	    return "<?-?>";
	}
	return &self.fragments[index as usize];
    }
}
