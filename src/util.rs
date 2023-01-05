// Copyright (C) 2022,23 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

// General utilities

pub const TRACING : bool = false;		// default: "false"
pub const LOGGING : bool = TRACING || true;	// default: "true" during development (we should add a log file...)
pub const WARNING : bool = true;		// default: "true"
pub const CARGO_TEST : bool = cfg!(test);

/// The operations listed here all fall back to println! when running unit tests.
/// This simplifies reporting and allows us to run "past" a fatal error to spot false positives in error checking
/// (I.e., we treat unit tests as ground truth, not the conditions around "error!()").


/// Low-level tracing, only enabled if TRACING is set.  For performance-critical sections (e.g. audio synthesis)
/// Typically enabled when debugging a very specific piece of functionality.
#[macro_export]
macro_rules! ptrace {
    ($($a:tt)*) => {
	if crate::util::TRACING {
	    if crate::util::CARGO_TEST {
		println!($($a)*)
	    } else {
		trace!($($a)*)
	    }
	}
    }
}

/// Debugging: Detail messages that are too spammy for logging and/or intended for module maintainers,
/// but don't affect performance to such a degree that we want to optimise them out by default.
/// Typically enabled on a per-module basis for debugging/high-level tracing.
/// Only enabled if LOGGING is set.
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

/// Info: One-time or uncommon messages that you want users to list and ask about in issue reports.
/// One-time messages may be lengthy (e.g., during decoding).
/// Typically enabled for validating.
/// Only enabled if LOGGING is set.
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

/// Warning: Something is off, but we have a safe fallback.
/// Should always be enabled (cf. WARNING)
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

/// Fatal error.
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
