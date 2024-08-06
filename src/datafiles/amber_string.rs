// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.


// Amberstar uses the Atari ST character set (only really visible via the 'ß' character in the German release)

// non-Unicode character or non-16-bit unicode character:
const NOCHAR: char = char::REPLACEMENT_CHARACTER;

const ATARI_ST_CODEPOINTS: [char; 256] = [
 '\0',        '\u{21e7}',  '\u{21e9}',  '\u{21e8}',  '\u{21e6}',  NOCHAR,      NOCHAR,      NOCHAR,      '\u{2713}',  NOCHAR,      NOCHAR,      '\u{266a}',  '\u{0012}',  '\u{0013}',  NOCHAR,      NOCHAR,
 NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,      '\u{018f}',  '\u{0021}',  NOCHAR,      NOCHAR,      NOCHAR,      NOCHAR,
 ' ',         '!',         '"',         '#',         '$',         '%',         '&',         '\'',        '(',         ')',         '*',         '+',         ',',         '-',         '.',         '/',
 '0',         '1',         '2',         '3',         '4',         '5',         '6',         '7',         '8',         '9',         ':',         ';',         '<',         '=',         '>',         '?',
 '@',         'A',         'B',         'C',         'D',         'E',         'F',         'G',         'H',         'I',         'J',         'K',         'L',         'M',         'N',         'O',
 'P',         'Q',         'R',         'S',         'T',         'U',         'V',         'W',         'X',         'Y',         'Z',         '[',         '\\',        ']',         '^',         '_',
 '`',         'a',         'b',         'c',         'd',         'e',         'f',         'g',         'h',         'i',         'j',         'k',         'l',         'm',         'n',         'o',
 'p',         'q',         'r',         's',         't',         'u',         'v',         'w',         'x',         'y',         'z',         '{',         '|',         '}',         '~',         '⌂',
 'Ç',         'ü',         'é',         'â',         'ä',         'à',         'å',         'ç',         'ê',         'ë',         'è',         'ï',         'î',         'ì',         'Ä',         'Å',
 'É',         'æ',         'Æ',         'ô',         'ö',         'ò',         'û',         'ù',         'ÿ',         'Ö',         'Ü',         '¢',         '£',         '¥',         'ß',         'ƒ',
 'á',         'í',         'ó',         'ú',         'ñ',         'Ñ',         'ª',         'º',         '¿',         '⌐',         '¬',         '½',         '¼',         '¡',         '«',         '»',
 'ã',         'õ',         'Ø',         'ø',         'œ',         'Œ',         'À',         'Ã',         'Õ',         '¨',         '´',         '†',         '¶',         '©',         '®',         '™',
 'ĳ',         'Ĳ',         '\u{05d0}',  '\u{05d1}',  '\u{05d2}',  '\u{05d3}',  '\u{05d4}',  '\u{05d5}',  '\u{05d6}',  '\u{05d7}',  '\u{05d8}',  '\u{05d9}',  '\u{05db}',  '\u{05dc}',  '\u{05de}',  '\u{05e0}',
 '\u{05e1}',  '\u{05e2}',  '\u{05e4}',  '\u{05e6}',  '\u{05e7}',  '\u{05e8}',  '\u{05e9}',  '\u{05ea}',  '\u{05df}',  '\u{05da}',  '\u{05dd}',  '\u{05e3}',  '\u{05e5}',  '§',         '∧',         '∞',
 'α',         'β',         'Γ',         'π',         'Σ',         'σ',         'µ',         'τ',         'Φ',         'Θ',         'Ω',         'δ',         '∮',         'ϕ',         '∈',         '∩',
 '≡',         '±',         '≥',         '≤',         '⌠',         '⌡',         '÷',         '≈',         '°',         '•',         '·',         '√',         'ⁿ',         '²',         '³',         '\u{00af}', ];

/// Converts all bytes in src to a string
pub fn from_bytes(src: &[u8]) -> String {
    src.iter().map(|&u| ATARI_ST_CODEPOINTS[u as usize]).collect()
}

/// Converts bytes in src to a string until it hits a zero byte
pub fn from_terminated_bytes(src: &[u8]) -> String {
    src.iter().take_while(|&u| *u > 0).map(|&u| ATARI_ST_CODEPOINTS[u as usize]).collect()
}

/// Panics if not found
pub fn to_byte(c: char) -> u8 {
    if c.is_ascii() {
	let code: u32 = c.into();
	return code as u8;
    }
    for pos in 128..ATARI_ST_CODEPOINTS.len() {
	if ATARI_ST_CODEPOINTS[pos] == c {
	    return pos as u8;
	}
    }
    panic!("Cannot convert '{c}' to Atari ST codepoint");
}

pub fn to_bytes(src: &str) -> Vec<u8> {
    src.chars().map(to_byte).collect()
}
