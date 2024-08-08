// Copyright (C) 2024 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::{iter::once, fmt::Display, collections::VecDeque};

pub struct Attr {
    name: String,
    value: AttributeValue
}

pub trait Usizeable {
    fn to_usize(&self) -> usize;
}
pub trait Isizeable {
    fn to_isize(&self) -> isize;
}
impl Usizeable for u8    { fn to_usize(&self) -> usize { *self as usize } }
impl Usizeable for u16   { fn to_usize(&self) -> usize { *self as usize } }
impl Usizeable for u32   { fn to_usize(&self) -> usize { *self as usize } }
impl Usizeable for u64   { fn to_usize(&self) -> usize { *self as usize } }
impl Usizeable for usize { fn to_usize(&self) -> usize { *self as usize } }

impl Isizeable for i8    { fn to_isize(&self) -> isize { *self as isize } }
impl Isizeable for i16   { fn to_isize(&self) -> isize { *self as isize } }
impl Isizeable for i32   { fn to_isize(&self) -> isize { *self as isize } }
impl Isizeable for i64   { fn to_isize(&self) -> isize { *self as isize } }
impl Isizeable for isize { fn to_isize(&self) -> isize { *self as isize } }

pub trait MaybeZero {
    fn is_zero(&self) -> bool;
}
impl<T> MaybeZero for T where T: Usizeable { fn is_zero(&self) -> bool { self.to_usize() == 0 } }
impl MaybeZero for i8   { fn is_zero(&self) -> bool { self.to_isize() == 0 } }
impl MaybeZero for i16  { fn is_zero(&self) -> bool { self.to_isize() == 0 } }
impl MaybeZero for i32  { fn is_zero(&self) -> bool { self.to_isize() == 0 } }
impl MaybeZero for i64  { fn is_zero(&self) -> bool { self.to_isize() == 0 } }
impl MaybeZero for isize { fn is_zero(&self) -> bool { self.to_isize() == 0 } }

pub struct MaybeIter {
    iter: Option<std::iter::Once<Attr>>,
}

impl Iterator for MaybeIter {
    type Item = Attr;

    fn next(&mut self) -> Option<Self::Item> {
	if let Some(ref mut it) = self.iter {
	    it.next()
	} else {
	    None
	}
    }
}

pub fn bool(name: &str, value: bool) -> std::iter::Once<Attr> {
    formatted(name, value)
}

pub fn usize<T: Usizeable>(name: &str, value: T) -> std::iter::Once<Attr> {
    once(Attr::usize(name, value)) }
pub fn usize_if_nonzero<T: Usizeable>(name: &str, value: T) -> MaybeIter {
    MaybeIter { iter: if value.is_zero() { None } else { Some(usize(name, value)) } } }

pub fn isize<T: Isizeable>(name: &str, value: T) -> std::iter::Once<Attr> {
    once(Attr::isize(name, value)) }
pub fn isize_if_nonzero<T: Isizeable+MaybeZero>(name: &str, value: T) -> MaybeIter {
    MaybeIter { iter: if value.is_zero() { None } else { Some(isize(name, value)) } } }

pub fn uhex<T: Usizeable>(name: &str, value: T) -> std::iter::Once<Attr> {
    once(Attr::uhex(name, value)) }
pub fn uhex_if_nonzero<T: Usizeable+MaybeZero>(name: &str, value: T) -> MaybeIter {
    MaybeIter { iter: if value.is_zero() { None } else { Some(uhex(name, value)) } } }
pub fn ihex<T: Isizeable>(name: &str, value: T) -> std::iter::Once<Attr> {
    once(Attr::ihex(name, value)) }
pub fn ihex_if_nonzero<T: Isizeable+MaybeZero>(name: &str, value: T) -> MaybeIter {
    MaybeIter { iter: if value.is_zero() { None } else { Some(ihex(name, value)) } } }

pub fn formatted<T>(name: &str, value: T) -> std::iter::Once<Attr>
where T: Display {
    once(Attr::formatted(name, value)) }
pub fn formatted_if<T>(t: bool, name: &str, value: T) -> MaybeIter
where T: Display {
    MaybeIter { iter: if t { None } else { Some(formatted(name, value)) } }
}

pub fn string(name: &str, value: &String) -> std::iter::Once<Attr> {
    once(Attr::string(name, value.clone())) }

// pub fn list(name: &str, value: String) -> std::iter::Once<Attr> {
//     once(Attr { name: name.to_string(),
// 		value: AttributeValue::String(value)
//     }) }

pub fn entity<T>(name: &str, value: &T) -> std::iter::Once<Attr>
    where T: Attributed {
    once(Attr::entity(name, value)) }

pub fn entity_it(name: &str, value: AttrIterator) -> std::iter::Once<Attr> {
    once(Attr { name: name.to_string(),
		value: AttributeValue::Entity(value)}) }


pub struct InlinedIterator {
    data: VecDeque<Attr>,
}

impl Iterator for InlinedIterator {
    type Item = Attr;

    fn next(&mut self) -> Option<Self::Item> {
	self.data.pop_front()
    }
}

pub fn inlined<T: Iterator<Item=Attr>>(it: &mut T) -> InlinedIterator {
    InlinedIterator {
	data: it.collect(),
    }
}

pub type AttrIterator = Box<dyn Iterator<Item=Attr>>;

pub enum AttributeValue {
    String(String),
    Count(usize, Box<AttributeValue>),
    Entity(AttrIterator),
}

pub fn count(count: usize, val: AttributeValue) -> AttributeValue {
    AttributeValue::Count(count, Box::new(val))
}

impl Attr {
    pub fn usize<T: Usizeable>(name: &str, value: T) -> Self {
	Attr { name: name.to_string(),
	       value: AttributeValue::String(format!("{}", value.to_usize()) )}
    }
    pub fn isize<T: Isizeable>(name: &str, value: T) -> Self {
	Attr { name: name.to_string(),
	       value: AttributeValue::String(format!("{}", value.to_isize()) )}
    }

    pub fn uhex<T: Usizeable>(name: &str, value: T) -> Self {
	Attr { name: name.to_string(),
		    value: AttributeValue::String(format!("0x{:x}", value.to_usize()))
	} }
    pub fn ihex<T: Isizeable>(name: &str, value: T) -> Self {
	Attr { name: name.to_string(),
		    value: AttributeValue::String(format!("0x{:x}", value.to_isize()))
	} }

    pub fn formatted<T>(name: &str, value: T) -> Self
    where T: Display {
	Attr { name: name.to_string(),
	       value: AttributeValue::String(format!("{value}"))
	} }

    pub fn string(name: &str, value: String) -> Self {
	Attr { name: name.to_string(),
		    value: AttributeValue::String(value)
    } }

    pub fn entity<T>(name: &str, value: &T) -> Self
    where T: Attributed {
	Attr { name: name.to_string(),
		    value: AttributeValue::Entity(value.attributes())
	}}

    pub fn count(self, num: usize) -> Self {
	if num == 0 {
	    self
	} else {
	    Attr{ name: self.name,
		  value: AttributeValue::Count(num, Box::new(self.value)) }
	}
    }
}


pub trait Attributed {
    fn attributes(&self) -> AttrIterator;
}

fn flatten_lines(v: Vec<(String, String)>) -> Vec<String> {
    let maxlen = v.iter()
	                .map(|(s, _)| s.len())
	                .fold(0, usize::max);
    v.iter()
        .map(|(s1, s2)| format!("{s1:width$}{s2}", width=maxlen))
        .collect()
}

fn collect_lines_v(value: AttributeValue) -> Vec<String> {
    match value {
	AttributeValue::String(s) => vec![format!("{s}")],
	AttributeValue::Count(n, sub) => {
	    let mut v = collect_lines_v(*sub);
	    for (i, s) in v.iter_mut().enumerate() {
		if i == 0 {
		    *s = format!("{n:4}x {}\t", *s);
		} else {
		    *s = format!("{:4}  {}\t", "", *s);
		}
	    }
	    v
	},
	AttributeValue::Entity(mut e) => {
	    flatten_lines(collect_lines(&mut e))
	},
    }
}

fn collect_lines(it: &mut AttrIterator) -> Vec<(String, String)> {
    let mut result = vec![];
    for Attr { name, value } in it {
	let mut newlines: Vec<(String, String)> =
	    collect_lines_v(value).iter().enumerate()
	                          .map(|(i, s)| (if i == 0 { format!("{name}  ") } else { String::new() },
					     s.clone()))
	                          .collect();
	result.append(&mut newlines);
    }
    result
}

pub fn print_rec(it: &mut AttrIterator, prefix: &str) {
    for s in flatten_lines(collect_lines(it)) {
	println!("{prefix}{s}");
    }
}
