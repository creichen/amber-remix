// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// DSP Architecture

pub mod frequency_range;
pub mod ringbuf;
pub mod writer;
pub mod linear;
pub mod stereo_mapper;
pub mod vtracker;
pub mod pcmsync;
pub mod crossfade_linear;
pub mod hermite;
pub mod mock_syncwriter;
pub mod mock_pcmwriter;
pub mod channel_sequencer;
