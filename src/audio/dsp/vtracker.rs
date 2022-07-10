// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

#[allow(unused)]
use log::{Level, log_enabled, trace, debug, info, warn, error};

use std::{sync::{Arc, Mutex}, ops::DerefMut, collections::VecDeque, fmt::Display};

pub const ENABLED : bool = true; // (not enforced): subsystems can choose to only track if this is "true"

const SPARKLINE : [char; 9] = ['-', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const MAX : u64 = 128;
const MAX_LEN : usize = 128;

#[derive(Clone, Copy)]
struct TrackerData {
    sum : u64,
    count : u64,
}

impl TrackerData {
    fn new() -> TrackerData { TrackerData { sum : 0, count : 0 } }

    fn char(&self) -> char {
	if self.count == 0 {
	    return ' ';
	}
	if self.count < 3 {
	    return '.';
	}
	let frac = self.sum as usize * SPARKLINE.len() / ((self.count * MAX) as usize);
	let index = usize::min(SPARKLINE.len() - 1, frac);
	return SPARKLINE[index];
    }
}

#[derive(Clone)]
pub struct TrackerSensor {
    r : Arc<Mutex<TrackerData>>,
}


impl TrackerSensor {
    pub fn new() -> TrackerSensor { TrackerSensor { r : Arc::new(Mutex::new(TrackerData::new())) } }

    pub fn add(&mut self, v : f32) {
	let mut guard = self.r.lock().unwrap();
	let d = guard.deref_mut();
	d.count += 1;
	d.sum += f32::abs(v * 256.0) as u64;
    }
    pub fn add_many(&mut self, v : f32, count : usize) {
	let mut guard = self.r.lock().unwrap();
	let d = guard.deref_mut();
	d.count += count as u64;
	d.sum += f32::abs(v * 256.0) as u64;
    }
    fn replace_accumulator(&mut self) -> TrackerData {
	let mut guard = self.r.lock().unwrap();
	let d = guard.deref_mut();
	let result = *d;
	*d = TrackerData::new();
	return result;
    }
}

pub struct Tracker {
    name : String,
    sensor : TrackerSensor,
    histogram : VecDeque<TrackerData>,
}

impl Tracker {
    pub fn new(name : String) -> Tracker {
	return Tracker {
	    name,
	    sensor : TrackerSensor::new(),
	    histogram : VecDeque::new(),
	}
    }

    pub fn sensor(&self) -> TrackerSensor {
	return self.sensor.clone();
    }

    /// Replace a sensor (useful if the tracker must be created in a different thread than the sensor)
    pub fn replace_tracker(&mut self, new_sensor : TrackerSensor) {
	self.sensor = new_sensor;
    }

    pub fn shift(&mut self) {
	let acc = self.sensor.replace_accumulator();
	self.histogram.push_front(acc);
	if self.histogram.len() > MAX_LEN {
	    self.histogram.pop_back();
	}
    }
}

impl Display for Tracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
	let mut s = "".to_string();
	for d in self.histogram.iter() {
	    s.push(d.char());
	}
	write!(f, "{:12} [{s}]", self.name)
    }
}
