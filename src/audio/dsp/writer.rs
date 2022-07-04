use crate::audio::dsp::frequency_range::Freq;
use crate::audio::dsp::frequency_range::FreqRange;

/// Writes fixed-frequency PCM data
pub trait PCMWriter {
    /// Output frequency
    fn frequency(&self) -> Freq;

    /// Write the specified number of samples to the given slice
    fn write_pcm(&mut self, output : &mut [f32]);
}

#[derive(PartialEq, Copy, Clone)]
pub enum FlexPCMResult {
    Wrote(usize),
    Flush, // Source reset: flush buffers and try to write again
    Silence,
}

/// Writes variable-frequency PCM data
pub trait FlexPCMWriter {
    /// Write the specified number of samples to the given slice.
    /// MSECS specifies the number of milliseconds for which to provide data.
    /// Returns the number of samples written.
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange, msecs : usize) -> FlexPCMResult;
}

impl<T : PCMWriter> FlexPCMWriter for T {
    fn write_flex_pcm(&mut self, output : &mut [f32], freqrange : &mut FreqRange, _msecs : usize) -> FlexPCMResult {
	*freqrange = FreqRange::new();
	freqrange.append(0, self.frequency());
	self.write_pcm(output);
	return FlexPCMResult::Wrote(output.len());
    }
}
