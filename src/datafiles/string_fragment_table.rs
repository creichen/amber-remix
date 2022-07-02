use codepage_strings::Coding;

pub struct StringFragmentTable {
    fragments : Vec<String>
}

impl StringFragmentTable {
    pub fn new(bytes : &[u8]) -> StringFragmentTable {
	let codepage = Coding::new(850).unwrap();

	let mut result = StringFragmentTable { fragments : vec!["<?-NUL-?>".to_string()] };
	let mut offset = 0;

	while offset < bytes.len() {
	    let str_len = bytes[offset] as usize;
	    if str_len == 0 {
		/* End of string table */
		break;
	    }
	    let str_vec = &bytes[offset+1..(offset+str_len)];
	    let str : String = codepage.decode_lossy(str_vec).to_string();
	    //println!("word[{}] @ {offset} = '{str}'", result.fragments.len());
	    if str == "#" {
		result.fragments.push("\n".to_string());
	    } else {
		result.fragments.push(str);
	    }
	    offset += str_len;
	};
	return result;
    }

    pub fn get(&self, index : u16) -> &str {
	if index as usize >= self.fragments.len() {
	    println!("Invalid string index {index}, max is {}", self.fragments.len() - 1);
	    return "<?-?>";
	}
	return &self.fragments[index as usize];
    }
}
