use crate::audio::dsp::frequency_range::Freq;
use crate::audio::dsp::frequency_range::FreqRange;

/// Writes fixed-frequency PCM data
trait PCMWriter {
    /// Output frequency
    fn frequency(&self) -> Freq;

    /// Write the specified number of samples to the given slice
    fn write_pcm(&self, output : &mut [f32]);
}

/// Writes variable-frequency PCM data
trait FlexPCMWriter {
    /// Write the specified number of samples to the given slice
    fn write_flex_pcm(&self, output : &mut [f32], freqrange : &mut FreqRange);
}

impl<T : PCMWriter> FlexPCMWriter for T {
    fn write_flex_pcm(&self, output : &mut [f32], freqrange : &mut FreqRange) {
	*freqrange = FreqRange::new();
	freqrange.append(0, self.frequency());
	self.write_pcm(output);
    }
}
