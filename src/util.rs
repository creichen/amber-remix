// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

// General utilities

pub const LOGGING : bool = false;
pub const WARNING : bool = true;
pub const CARGO_TEST : bool = cfg!(test);

#[macro_export]
macro_rules! ptrace {
    ($($a:tt)*) => {
	if crate::util::LOGGING {
	    if crate::util::CARGO_TEST {
		println!($($a)*)
	    } else {
		trace!($($a)*)
	    }
	}
    }
}

#[macro_export]
macro_rules! pdebug {
    ($($a:tt)*) => {
	if crate::util::LOGGING {
	    if crate::util::CARGO_TEST {
		println!($($a)*)
	    } else {
		debug!($($a)*)
	    }
	}
    }
}

#[macro_export]
macro_rules! pinfo {
    ($($a:tt)*) => {
	if crate::util::LOGGING {
	    if crate::util::CARGO_TEST {
		println!($($a)*)
	    } else {
		info!($($a)*)
	    }
	}
    }
}

#[macro_export]
macro_rules! pwarn {
    ($($a:tt)*) => {
	if crate::util::WARNING {
	    if crate::util::CARGO_TEST {
		println!($($a)*)
	    } else {
		warn!($($a)*)
	    }
	}
    }
}

#[macro_export]
macro_rules! perror {
    ($($a:tt)*) => {
	if crate::util::CARGO_TEST {
	    println!($($a)*)
	} else {
	    error!($($a)*)
	}
    }
}

pub trait IndexLen<T> {
    fn len(&self) -> usize;
    fn get(&self, pos : usize) -> T;
}

impl<T> IndexLen<T> for &[T] where T : Copy {
    fn len(&self) -> usize {
	return <[T]>::len(self);
    }

    fn get(&self, pos : usize) -> T {
	return self[pos];
    }
}

impl<T> IndexLen<T> for &Vec<T> where T : Copy {
    fn len(&self) -> usize {
	return Vec::len(self);
    }

    fn get(&self, pos : usize) -> T {
	return self[pos];
    }
}
