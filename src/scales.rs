#![allow(dead_code)]
use std::mem::transmute;

#[inline(always)]
pub fn unitnorm(x: f32) -> f32 {
	let mut i: u32 = unsafe {transmute(x)};
	i = (i & 0x80000000) | 0x3f800000;
	unsafe {transmute(i)}
}

/// * Convert dB to gain
#[inline(always)]
#[allow(non_snake_case)]
pub fn todB(x: &f32) -> f32 {
	let mut i: u32 = unsafe {transmute(*x)};
	i &= 0x7FFFFFFF;
	i as f32 * 7.17711438e-7 - 764.6161886
}

/// * Convert gain to dB
#[inline(always)]
#[allow(non_snake_case)]
pub fn fromdB(x: f32) -> f32 {
    (x * 0.11512925).exp()
}

/* The bark scale equations are approximations, since the original
   table was somewhat hand rolled.  The below are chosen to have the
   best possible fit to the rolled tables, thus their somewhat odd
   appearance (these are more accurate and over a longer range than
   the oft-quoted bark equations found in the texts I have).  The
   approximations are valid from 0 - 30kHz (nyquist) or so.

   all f in Hz, z in Bark */

#[inline(always)]
#[allow(non_snake_case)]
pub fn toBARK(n: f32) -> f32 {
	13.1 * (n * 0.00074).atan()+2.24 * (n * n * 1.85e-8).atan() + 1e-4 * n
}

#[inline(always)]
#[allow(non_snake_case)]
pub fn fromBARK(z: f32) -> f32 {
	102.0 * z - 2.0 * z.powf(2.0) + 0.4 * z.powf(3.0) + 1.46_f32.powf(z) - 1.0
}

#[inline(always)]
#[allow(non_snake_case)]
pub fn toMEL(n: f32) -> f32 {
	(1.0 + n * 0.001).ln() * 1442.695
}

#[inline(always)]
#[allow(non_snake_case)]
pub fn fromMEL(m: f32) -> f32 {
	1000.0 * (m / 1442.695).exp() - 1000.0
}

/* Frequency to octave.  We arbitrarily declare 63.5 Hz to be octave
   0.0 */

#[inline(always)]
#[allow(non_snake_case)]
pub fn toOC(n: f32) -> f32 {
	n.ln() * 1.442695 - 5.965784
}

#[inline(always)]
#[allow(non_snake_case)]
pub fn fromOC(o: f32) -> f32 {
	((o + 5.965784) * 0.693147).exp()
}
