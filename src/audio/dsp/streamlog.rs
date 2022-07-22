// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Stream processing loggers

use std::sync::{Mutex, Arc};

pub trait StreamLogClient {
    fn set_logger(&mut self, logger : ArcStreamLogger);
}

pub trait StreamLogger : Send + Sync {
    fn log(&mut self, subsystem : &'static str, category : &'static str, message : String);
}

pub type ArcStreamLogger = Arc<Mutex<dyn StreamLogger>>;

impl StreamLogger for () {
    fn log(&mut self, _subsystem : &'static str, _category : &'static str, _message : String) {
    }
}

pub fn dummy() -> ArcStreamLogger {
    return Arc::new(Mutex::new(()));
}

pub fn log(logger : &ArcStreamLogger, subsystem : &'static str, category : &'static str, message : String) {
    let mut guard = logger.lock().unwrap();
    guard.log(subsystem, category, message);
}
