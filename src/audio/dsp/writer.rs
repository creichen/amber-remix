use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;

use crate::audio::dsp::frequency_range::Freq;
use crate::audio::dsp::frequency_range::FreqRange;

// ================================================================================

/// A monotonically increasing time designator
/// We use time slices to indicate ticks (0.02s intervals), but that interpretation is arbitrary.
type Timeslice = usize;

/// Writes fixed-frequency PCM data
pub trait PCMWriter {
    /// Output frequency
    fn frequency(&self) -> Freq;

    /// Write the specified number of samples to the given slice
    fn write_pcm(&mut self, output : &mut [f32]);
}

type ArcWriter = Arc<Mutex<dyn PCMWriter>>;

// ================================================================================

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum FlexPCMResult {
    /// Specify number of samples written and optionally whether writer is ready for next time slice
    Wrote(usize, Option<Timeslice>),
    Flush, // Source reset: flush buffers, set current time slice to 0, try to write again
    Silence,
}

/// Writes variable-frequency PCM data
pub trait FlexPCMWriter {
    /// Write the specified number of samples to the given slice.
    /// TIME specifies the timeslice for which we should generate data.
    /// The FlexPCMWriter might thus be asked to produce audio beyond what it thinks the length of the current time slice is,
    /// but that decision is up to the consumer below (which will discard "ueseless" samples).
    /// Returns the number of samples written.
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange, time : Timeslice) -> FlexPCMResult;
}

// ================================================================================

/// Synchronising multiple writers across time slices
///
/// Synchronising multiple (Flex) writers is handled by an external
pub trait PCMAudioSyncWriter : PCMWriter {
    ///
    fn writer_sync_state(&self) -> (usize, Timeslice);

    /// +
    fn writer_sync_forward(&mut self, samples: usize, slice: Timeslice);
}

type ArcSyncWriter = Arc<Mutex<dyn PCMAudioSyncWriter>>;

type SyncID = usize;

pub trait WriterSynchronizerTrait {

    ///
    fn register(&mut self) -> SyncID;

    /// 
    fn report_timeslice_ready(&self, SyncID, usize, Timeslice) -> (usize, Timeslice);

    /// +
    fn writer_sync_forward(&mut self, samples: usize, slice: Timeslice);
}


// ================================================================================



struct WriterState {
    timeslice : Timeslice,
    written : usize,
    writer : ArcSyncWriter,
}

struct WriterSync {
    writer_states : Vec<WriterState>,
}

impl WriterSync {
    fn synchroniser_for(self : Rc<WriterSync>, writer: ArcSyncWriter) -> WriterSyncFwd {
	let guard = writer.lock().unwrap();
	let freq = guard.frequency();
	return WriterSyncFwd {
	    wsync : self,
	    writer,
	    freq,
	};
    }

    fn requesting(&mut self, samples : usize) -> usize {
	// check all writers for their state
	// 
    }
}

struct WriterSyncFwd {
    wsync : Rc<WriterSync>,
    writer : ArcSyncWriter,
    freq : Freq,
}

impl WriterSyncFwd {
}

impl PCMWriter for WriterSyncFwd {
    fn frequency(&self) -> Freq {
	return self.freq;
    }

    fn write_pcm(&mut self, output : &mut [f32]) {
	let result = self.wsync.requesting(output.len());
    }
}
