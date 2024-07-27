#[allow(unused)]

use rustfft::{FftPlanner, num_complex::Complex};
use std::f32::consts::PI;

// Parameters
const BLEP_TABLE_SIZE: usize = 4096;
const SAMPLE_RATE: f32 = 25000.0;
const CUTOFF_FREQ: f32 = 12500.0;

fn hann_window(n: usize, size: usize) -> f32 {
    0.5 * (1.0 - (2.0 * PI * n as f32 / (size - 1) as f32).cos())
}

// Generate BLEP table
fn generate_blep_table() -> Vec<f32> {
    let mut table = vec![0.0; BLEP_TABLE_SIZE];
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(BLEP_TABLE_SIZE);
    let ifft = planner.plan_fft_inverse(BLEP_TABLE_SIZE);

    // Step function in the time domain
    let mut step = vec![Complex::new(0.0, 0.0); BLEP_TABLE_SIZE];
    for i in 0..BLEP_TABLE_SIZE / 2 {
        step[i].re = 1.0;
    }

    // Transform step function to frequency domain
    fft.process(&mut step);

    // Apply the low-pass filter to get the BLEP frequency domain
    for i in 0..BLEP_TABLE_SIZE / 2 {
        let freq = i as f32 * SAMPLE_RATE / BLEP_TABLE_SIZE as f32;
        if freq <= CUTOFF_FREQ {
            step[i] *= Complex::new(hann_window(i, BLEP_TABLE_SIZE), 0.0);
        } else {
	    step[i] = Complex::new(0.0, 0.0);
	}
    }

    // Map to time domain
    ifft.process(&mut step);
    let mut max_value = 0.0;
    for i in 0..BLEP_TABLE_SIZE {
	let v = step[i].re;
        table[i] = v;
	max_value = f32::max(max_value, v);
    }

    // Normalise
    for i in 0..BLEP_TABLE_SIZE {
        table[i] /= max_value;
    }

    table
}

pub struct BLEP {
    table: Vec<f32>,
}

impl BLEP {
    pub fn new() -> Self {
	BLEP {
	    table: generate_blep_table(),
	}
    }
    pub fn apply_blep(&self, output: &mut [f32],
		  position: usize, pcm: f32) {
	let table_size = self.table.len();
	for i in 0..table_size {
            if position + i < output.len() {
		output[position + i] += pcm * self.table[i];
            }
	}
    }
}
