// Copyright (C) 2022 Christoph Reichenbach (creichen@gmail.com)
// Licenced under the GNU General Public Licence, v3.  Please refer to the file "COPYING" for details.

/// Fit PCM data

#[derive(Copy, Clone)]
pub struct PCMFit {
    pub(super) f : [f32; 3],
}

impl PCMFit {
    pub fn new<'a>(slice : &'a [f32], offset : usize) -> PCMFit {
	if slice.len() == 0 {
	    return PCMFit { f : [0.0, 0.0, 0.0] };
	}
	let last = slice.len() - 1;
	let z = slice[offset];
	let mut result = PCMFit {
	    f : [z, 0.0, 0.0],
	};
	if offset > 1 && offset + 2 < last {
	    // Five-point method
	    let m2 = slice[offset - 2];
	    let m1 = slice[offset - 1];
	    let p1 = slice[offset + 1];
	    let p2 = slice[offset + 2];
	    result.f[1] = (8.0 * (p1 - m1) + m2 - p2) / 12.0;
	    result.f[2] = (16.0 * (p1 + m1) - m2 - p2 - (30.0 * z)) / 12.0;
	} else if offset > 0 && offset + 1 <= last {
	    // Three-point method
	    let m1 = slice[offset - 1];
	    let p1 = slice[offset + 1];
	    result.f[1] = (p1 - m1) / 2.0;
	    result.f[2] = p1 + m1 - 2.0 * z;
	}
	return result;
    }

    /// Returns a distance score; lower means better fit, zero is perfect fit.
    pub fn distance(&self, other : &PCMFit) -> f32 {
	let d0 = f32::abs(other.f[0] - self.f[0]);
	let d1 = f32::abs(other.f[1] - self.f[1]);
	let d2 = f32::abs(other.f[2] - self.f[2]);

	return     1.0 * d0*d0
		 + 2.0 * d1*d1
		 + 1.0 * d2*d2;
    }
}


#[cfg(test)]
mod test {
    use super::PCMFit;

    #[cfg(test)]
    fn assert_eq_epsilon(l : f32, r : f32) {
	if f32::abs(l - r) > 0.001 {
	    assert_eq!(l, r);
	}
    }

    #[test]
    pub fn test_deriv1() {
	let d = [0.5];
	let fit0 = PCMFit::new(&d, 0);
	assert_eq!(0.5, fit0.f[0]);
	assert_eq!(0.0, fit0.f[1]);
	assert_eq!(0.0, fit0.f[2]);
    }

    #[test]
    pub fn test_deriv1_boundary() {
	let d = [0.5, 1.0];

	let fit0 = PCMFit::new(&d, 0);
	assert_eq!(0.5, fit0.f[0]);
	assert_eq!(0.0, fit0.f[1]);
	assert_eq!(0.0, fit0.f[2]);

	let fit1 = PCMFit::new(&d, 1);
	assert_eq!(1.0, fit1.f[0]);
	assert_eq!(0.0, fit1.f[1]);
	assert_eq!(0.0, fit1.f[2]);
    }

    #[test]
    pub fn test_deriv3() {
	let d = [0.4, 0.5, 0.8];
	let fit = PCMFit::new(&d, 1);
	assert_eq!(0.5, fit.f[0]);
	assert_eq_epsilon(0.2, fit.f[1]);
	assert_eq_epsilon(0.2, fit.f[2]);
    }

    #[test]
    pub fn test_deriv3_boundary() {
	let d = [0.3, 0.5, 0.7, 0.0];

	let fit = PCMFit::new(&d, 1);
	assert_eq!(0.5, fit.f[0]);
	assert_eq_epsilon(0.2, fit.f[1]);
	assert_eq_epsilon(0.0, fit.f[2]);

	let fit = PCMFit::new(&d, 2);
	assert_eq!(0.7, fit.f[0]);
	assert_eq_epsilon(-0.25, fit.f[1]);
	assert_eq_epsilon(-0.9, fit.f[2]);
    }

    #[test]
    pub fn test_deriv5() {
	// f(x) = 0.3 -0.2x - 0.1x^2
	let d = [0.3, 0.4, 0.3, 0.0, -0.5];
	let fit = PCMFit::new(&d, 2);
	assert_eq!(0.3, fit.f[0]);
	assert_eq_epsilon(-0.2, fit.f[1]);
	//assert_eq_epsilon(-0.1, fit.f[2]);
	assert_eq_epsilon(-0.2, fit.f[2]); // expected imprecision
    }

    #[test]
    pub fn test_deriv5_boundary() {
	// f(x) = 0.4 + 0.2x - 0.1x^2
	let d = [-0.4, 0.1, 0.4, 0.5, 0.4, 0.0];

	let fit = PCMFit::new(&d, 2);
	assert_eq!(0.4, fit.f[0]);
	assert_eq_epsilon(0.2, fit.f[1]);
	//assert_eq_epsilon(-0.1, fit.f[2]);
	assert_eq_epsilon(-0.2, fit.f[2]); // expected imprecision

	// f(x) = -0.1 + -0.15x + 0.2x^2
	let d = [0.0, 1.0, 0.25, -0.1, -0.05, 0.4];
	let fit = PCMFit::new(&d, 3);
	assert_eq!(-0.1, fit.f[0]);
	assert_eq_epsilon(-0.15, fit.f[1]);
	//assert_eq_epsilon(0.2, fit.f[2]);
	assert_eq_epsilon(0.4, fit.f[2]); // expected imprecision
    }

    #[test]
    pub fn test_similarity() {
	let d0 = [0.3, 0.4, 0.3, 0.0, -0.5];
	let d1 = [0.3, 0.4, 0.3, 0.2, 0.0];
	let d2 = [0.3, 0.4, 0.5, 0.4, 0.3];

	let fit0 = PCMFit::new(&d0, 2);
	let fit1 = PCMFit::new(&d1, 2);
	let fit2 = PCMFit::new(&d2, 2);

	assert_eq_epsilon(0.0, fit0.distance(&fit0));
	assert_eq_epsilon(0.0, fit1.distance(&fit1));
	assert_eq_epsilon(0.0, fit2.distance(&fit2));

	assert!(fit0.distance(&fit1) < fit0.distance(&fit2));

	assert_eq_epsilon(fit1.distance(&fit2), fit2.distance(&fit1));
	assert_eq_epsilon(fit1.distance(&fit0), fit0.distance(&fit1));
	assert_eq_epsilon(fit2.distance(&fit0), fit0.distance(&fit2));
    }
}
