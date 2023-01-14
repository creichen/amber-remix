// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

use std::{cell::RefCell, rc::Rc};
use crate::audio::Freq;

use super::writer::{PCMWriter, FrequencyTrait, RcPCMWriter};


struct MockPCMWriter {
}

impl FrequencyTrait for MockPCMWriter {
    fn frequency(&self) -> Freq {
	return 42;
    }
}

impl PCMWriter for MockPCMWriter {
    fn write_pcm(&mut self, output : &mut [f32]) {
	output.fill(0.0);
    }
}

pub fn mock_pw() -> RcPCMWriter {
    return Rc::new(RefCell::new(MockPCMWriter {
    }));
}
