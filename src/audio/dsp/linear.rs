/// Linearly interpolating remixer
///
/// Not expected to produce particularly high-quality output
struct SampleState {
    in_freq : u32,
    out_freq : u32,         // output buffer frequency

    // index into sample data
    sample_pos_int : usize,   // integral part
    sample_pos_fract : f32, // fractional part (nominator; the denominator is out_freq)
}

impl SampleState {
    fn new(in_freq : u32, out_freq : u32) -> SampleState {
	SampleState {
	    in_freq,
	    out_freq,
	    sample_pos_int : 0,
	    sample_pos_fract : 0.0,
	}
    }

    fn resample(&mut self, outbuf : &mut [f32], inbuf : &[f32]) {
	let sample_len = inbuf.len();

	let mut pos = self.sample_pos_int;

	// fractional position counter
	let mut fpos_nom = self.sample_pos_fract as f32;
	let fpos_nom_inc_total = self.in_freq;
	let fpos_denom = self.out_freq as f32;
	let pos_inc = (fpos_nom_inc_total / self.out_freq) as usize;
	let fpos_nom_inc = (fpos_nom_inc_total % self.out_freq) as f32;

	for out in outbuf.iter_mut() {
	    // Linear interpolation
	    let sample_v_current = inbuf[pos];

	    let sample_v_next = if pos + 1 == sample_len  { sample_v_current } else { inbuf[pos + 1] };

	    let sample_v_current_fragment = sample_v_current * (fpos_denom - fpos_nom);
	    let sample_v_next_fragment = sample_v_next * fpos_nom;

	    let sample_v = (sample_v_current_fragment + sample_v_next_fragment) / fpos_denom;

	    *out += sample_v;

	    pos += pos_inc;
	    fpos_nom = fpos_nom + fpos_nom_inc;
	    if fpos_nom >= fpos_denom {
		fpos_nom -= fpos_denom;
		pos += 1;
	    }
	}
	self.sample_pos_int = pos;
	self.sample_pos_fract = fpos_nom;
    }
}

// ----------------------------------------

#[cfg(test)]
#[test]
fn copy() {
    let mut outbuf : [f32; 5] = [0.0; 5];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 20.0,
		 100.0,
		 10.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn upsample_double() {
    let mut outbuf : [f32; 10] = [0.0; 10];
    let inbuf = vec![5.0, 20.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 200);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [5.0,
		 12.5,
		 20.0,
		 60.0,
		 100.0,
		 55.0,
		 10.0,
		 25.0,
		 40.0,
		 40.0,],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn upsample_triple() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[..], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn upsample_incremental() {
    let mut outbuf : [f32; 15] = [0.0; 15];
    let inbuf = vec![10.0, 40.0, 100.0, 10.0, 40.0];
    let mut sstate = SampleState::new(100, 300);
    assert_eq!([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
		 &outbuf[..]);

    sstate.resample(&mut outbuf[2..11], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 0.0, 0.0, 0.0, 0.0 ],
		 &outbuf[..]);

    sstate.resample(&mut outbuf[11..15], &inbuf);

    assert_eq!( [10.0,
		 20.0,
		 30.0,
		 40.0,
		 60.0,
		 80.0,
		 100.0,
		 70.0,
		 40.0,
		 10.0,
		 20.0,
		 30.0,
		 40.0,
		 40.0,
		 40.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn downsample_double() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(100, 50);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..3], &inbuf);
    assert_eq!( [10.0,
		 30.0,
		 50.0,
		 0.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn downsample_one_point_five() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

#[cfg(test)]
#[test]
fn downsample_one_point_five_incremental() {
    let mut outbuf : [f32; 4] = [0.0; 4];
    let inbuf = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut sstate = SampleState::new(150, 100);
    assert_eq!([0.0, 0.0, 0.0, 0.0],
	       &outbuf[..]);
    sstate.resample(&mut outbuf[0..2], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 0.0,
		 0.0 ],
		 &outbuf[..]);
    sstate.resample(&mut outbuf[2..4], &inbuf);
    assert_eq!( [10.0,
		 25.0,
		 40.0,
		 55.0 ],
		 &outbuf[..]);
}

// ================================================================================
