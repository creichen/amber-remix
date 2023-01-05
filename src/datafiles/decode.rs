// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

pub fn u16(vec : &[u8], offset : usize) -> u16 {
    let hi = vec[offset] as u16;
    let lo = vec[offset + 1] as u16;
    return hi << 8 | lo;
}

pub fn u32(vec : &[u8], offset : usize) -> u32 {
    let hi = u16(vec, offset) as u32;
    let lo = u16(vec, offset + 2) as u32;
    return hi << 16 | lo;
}
