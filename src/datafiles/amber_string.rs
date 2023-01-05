// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use codepage_strings::Coding;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref CODEPAGE : Coding = {
	Coding::new(850).unwrap()
    };
}

pub fn from_bytes(src : &[u8]) -> String {
    CODEPAGE.decode_lossy(src).to_string()
}
