use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::dsp::frequency_range::Freq;
use crate::audio::dsp::frequency_range::FreqRange;

// ================================================================================
// Fixed-frequency writers that do not need synchronisation

pub trait FrequencyTrait {
    /// Associated frequency, e.g., output frequency for writers
    fn frequency(&self) -> Freq;
}

/// Writes fixed-frequency PCM data
pub trait PCMWriter : FrequencyTrait {
    /// Write the specified number of samples to the given slice
    fn write_pcm(&mut self, output : &mut [f32]);
}

pub type RcPCMWriter = Rc<RefCell<dyn PCMWriter>>;

// ================================================================================
// Fixed-frequency writers that can synchronise on timeslice boundaries

/// A monotonically increasing time designator
/// We use time slices to indicate ticks (0.02s intervals), but that interpretation is arbitrary.
pub type Timeslice = usize;

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum SyncPCMResult {
    /// Specify number of samples written and optionally whether writer is ready for next time slice
    Wrote(usize, Option<Timeslice>),
    /// Source reset: flush buffers, set current time slice to 0, try to write again
    Flush,
}

/// Writes fixed-frequency PCM data
pub trait PCMSyncWriter : FrequencyTrait {
    /// Write the specified number of samples to the given slice
    /// Will write that many bytes except for two situations:
    /// - Encountered time slice change (in which case the first "Wrote()" after the time slice
    ///   change may report fewer bytes than requested, but later calls must no
    /// - Flush
    fn write_sync_pcm(&mut self, output : &mut [f32]) -> SyncPCMResult;

    /// Permit the writer to advance to the next time slice (as specified)
    fn advance_sync(&mut self, timeslice : Timeslice);
}

pub type RcSyncWriter = Rc<RefCell<dyn PCMSyncWriter>>;

// ================================================================================
// Flexible-frequency writers that must synchronise on timeslice boundaries

/// Writes variable-frequency PCM data
pub trait PCMFlexWriter {
    /// Write the specified number of samples to the given slice.
    /// TIME specifies the timeslice for which we should generate data.
    /// The FlexPCMWriter might thus be asked to produce audio beyond what it thinks the length of the current time slice is,
    /// but that decision is up to the consumer below (which will discard "ueseless" samples).
    /// Returns the number of samples written.
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange) -> SyncPCMResult;

    /// Permit the writer to advance to the next time slice (as specified)
    fn advance_sync(&mut self, timeslice : Timeslice);
}

// ================================================================================

/// Synchronise multiple PCMSyncWriters/ArcSyncWriters across time slices
pub trait PCMSyncBarrier {
    /// Register an ArcSyncWriter for synchronisation
    fn sync(&mut self, writer : RcSyncWriter) -> RcPCMWriter;
}

pub type RcSyncBarrier = Rc<RefCell<dyn PCMSyncBarrier>>;
