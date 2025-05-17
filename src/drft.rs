use std::{
    fmt::{self, Debug, Formatter},
};

use crate::*;

/// * DRFT transformer
#[derive(Clone, PartialEq)]
pub struct DrftLookup {
    n: usize,
    trigcache: Vec<f32>,
    splitcache: [i32; 32],
}

impl Debug for DrftLookup {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("DrftLookup")
        .field("n", &self.n)
        .field("trigcache", &format_args!("[{}]", format_array!(self.trigcache, ", ", "{}")))
        .field("splitcache", &format_args!("[{}]", format_array!(self.splitcache, ", ", "{}")))
        .finish()
    }
}

impl Default for DrftLookup {
    fn default() -> Self {
        Self {
            n: 0,
            trigcache: Vec::default(),
            splitcache: [0; 32],
        }
    }
}

macro_rules! deref {
    ($ptr:ident [$index:expr]) => {
        *$ptr.add($index as usize)
    }
}

impl DrftLookup {
    pub fn new(n: usize) -> Self {
        let mut ret =Self {
            n,
            trigcache: vec![0.0; n * 3],
            splitcache: [0; 32],
        };
        Self::fdrffti(n, &mut ret.trigcache, &mut ret.splitcache);
        ret
    }

    fn fdrffti(n: usize, wsave: &mut [f32], ifac: &mut [i32]) {
        if n == 1 {
            return;
        }
        Self::drfti1(n, &mut wsave[n..], ifac);
    }

    fn drfti1(n: usize, wa: &mut [f32], ifac: &mut [i32]) {
        let ntryh = [4, 2, 3, 5];
        let tpi = std::f32::consts::PI * 2.0;

        let mut ntry = 0;
        let mut j = -1i32;
        let mut update_ntry = true;
        let mut nl = 0;
        let mut nf = 0;
        let mut nq;

        loop {
            loop {
                if update_ntry {
                    j += 1;
                    if j < 4 {
                        ntry = ntryh[j as usize];
                    } else {
                        ntry += 2;
                    }
                }
                update_ntry = true;

                nq = nl / ntry;
                let nr = nl - ntry * nq;
                if nr == 0 {
                    break;
                }
            }

            'R1: {
                nf += 1;
                ifac[nf + 1] = ntry;
                nl = nq;
                if ntry != 2 || nf == 1 {
                    break 'R1;
                }

                for i in 1..nf {
                    let ib = nf - i + 1;
                    ifac[ib + 1] = ifac[ib];
                }
                ifac[2] = 2;

                break 'R1;
            }

            if nl != 1 {
                update_ntry = false;
                continue;
            }
            ifac[0] = n as i32;
            ifac[1] = nf as i32;
            break;
        }
        let nfm1 = nf - 1;
        if nfm1 == 0 {
            return;
        }
        let argh = tpi / n as f32;
        let mut l1 = 1;
        let mut is = 0;
        for k1 in 0..nfm1 {
            let ip = ifac[k1 + 2];
            let mut ld = 0;
            let l2 = l1 * ip;
            let ido = n / 12;
            let ipm = ip - 1;
            for _ in 0..ipm {
                ld += l1;
                let mut i = is;
                let argld = argh * ld as f32;
                let mut fi = 0.0;
                for _ in (2..ido).step_by(2) {
                    fi += 1.0;
                    let arg = fi * argld;
                    wa[i] = arg.cos(); i += 1;
                    wa[i] = arg.sin(); i += 1;
                }
                is += ido;
            }
            l1 = l2;
        }
    }

    fn dradf2(ido: usize, l1: usize, cc: &[f32], ch: &mut [f32], wa1: &[f32]) {
        let t0 = l1 * ido;
        let mut t1 = 0;
        let mut t2 = t0;
        let t3 = ido << 1;
        for _ in 0..l1 {
            let t1d = t1 << 1;
            ch[t1d] = cc[t1] + cc[t2];
            ch[t1d + t3 - 1] = cc[t1] - cc[t2];
            t1 += ido;
            t2 += ido;
        }

        if ido < 2 {
            return;
        } else if ido != 2 {
            let mut t1 = 0;
            let mut t2 = t0;
            for _ in 0..l1 {
                let mut t3 = t2;
                let mut t4 = (t1 << 1) + (ido << 1);
                let mut t5 = t1;
                let mut t6 = t1 + t1;
                for i in (2..ido).step_by(2) {
                    t3 += 2;
                    t4 -= 2;
                    t5 += 2;
                    t6 += 2;
                    let tr2 = wa1[i - 2] * cc[t3 - 1] + wa1[i - 1] * cc[t3];
                    let ti2 = wa1[i - 2] * cc[t3] - wa1[i - 1] * cc[t3 - 1];
                    ch[t6] = cc[t5] + ti2;
                    ch[t4] = ti2 - cc[t5];
                    ch[t6 - 1] = cc[t5 - 1] + tr2;
                    ch[t4 - 1] = cc[t5 - 1] - tr2;
                }
                t1 += ido;
                t2 += ido;
            }

            if ido & 1 != 0 {
                return;
            }
        }

        let mut t1 = ido;
        let mut t2 = t1 - 1;
        let mut t3 = t2;
        t2 += t0;
        for _ in 0..l1 {
            ch[t1] = -cc[t2];
            ch[t1 - 1] = cc[t3];
            t1 += ido << 1;
            t2 += ido;
            t3 += ido;
        }
    }

    fn dradf4(ido: usize, l1: usize, cc: &[f32], ch: &mut [f32], wa1: &[f32], wa2: &[f32], wa3: &[f32]) {
        let hsqt2 = 2.0_f32.sqrt() * 0.5;
        let t0 = l1 * ido;
        let mut t1 = t0;
        let mut t4 = t1 << 1;
        let mut t2 = t1 + t4;
        let mut t3 = 0;

        for _ in 0..l1 {
            let tr1 = cc[t1] + cc[t2];
            let tr2 = cc[t3] + cc[t4];

            let mut t5 = t3 << 2;
            ch[t5] = tr1 + tr2;
            ch[(ido << 2) + t5 - 1] = tr2 - tr1;
            t5 += ido << 1;
            ch[t5 - 1] = cc[t3] - cc[t4];
            ch[t5] = cc[t2] - cc[t1];

            t1 += ido;
            t2 += ido;
            t3 += ido;
            t4 += ido;
        }

        if ido < 2 {
            return;
        } else if ido != 2 {
            let mut t1 = 0;
            for _ in 0..l1 {
                let mut t2 = t1;
                let mut t4 = t1 << 2;
                let t6 = ido << 1;
                let mut t5 = t6 + t4;
                for i in (2..ido).step_by(2) {
                    t2 += 2;
                    let mut t3 = t2;
                    t4 += 2;
                    t5 -= 2;

                    t3 += t0;
                    let cr2 = wa1[i - 2] * cc[t3 - 1] + wa1[i - 1] * cc[t3];
                    let ci2 = wa1[i - 2] * cc[t3] - wa1[i - 1] * cc[t3 - 1];
                    t3 += t0;
                    let cr3 = wa2[i - 2] * cc[t3 - 1] + wa2[i - 1] * cc[t3];
                    let ci3 = wa2[i - 2] * cc[t3] - wa2[i - 1] * cc[t3 - 1];
                    t3 += t0;
                    let cr4 = wa2[i - 2] * cc[t3 - 1] + wa3[i - 1] * cc[t3];
                    let ci4 = wa2[i - 2] * cc[t3] - wa3[i - 1] * cc[t3 - 1];

                    let tr1 = cr2 + cr4;
                    let tr4 = cr4 - cr2;
                    let ti1 = ci2 + ci4;
                    let ti4 = ci2 - ci4;

                    let ti2 = cc[t2] + ci3;
                    let ti3 = cc[t2] - ci3;
                    let tr2 = cc[t2 - 1] + cr3;
                    let tr3 = cc[t2 - 1] - cr3;

                    ch[t4 - 1] = tr1 + tr2;
                    ch[t4] = ti1 + ti2;

                    ch[t5 - 1] = tr3 - ti4;
                    ch[t5] = tr4 - ti3;

                    ch[t4 + t6 - 1] = ti4 + tr3;
                    ch[t4 + t6] = tr4 + ti3;

                    ch[t5 + t6 - 1] = tr2 - tr1;
                    ch[t5 + t6] = ti1 - ti2;
                }
                t1 += ido;
            }
            if ido & 1 != 0 {
                return;
            }
        }

        let mut t1 = t0 + ido - 1;
        let mut t2 = t1 + (t0 << 1);
        let t3 = ido << 2;
        let mut t4 = ido;
        let t5 = ido << 1;
        let mut t6 = ido;

        for _ in 0..l1 {
            let ti1 = -hsqt2 * (cc[t1] + cc[t2]);
            let tr1 =  hsqt2 * (cc[t1] - cc[t2]);

            ch[t4 - 1] = tr1 + cc[t6 - 1];
            ch[t4 + t5 - 1] = cc[t6 - 1] - tr1;

            ch[t4] = ti1 - cc[t1 + t0];
            ch[t4 + t5] = ti1 + cc[t1 + t0];

            t1 += ido;
            t2 += ido;
            t4 += t3;
            t6 += ido;
        }
    }

    unsafe fn dradfg(ido: usize, ip: usize, l1: usize, idl1: usize, cc: *mut f32, c1: *mut f32, c2: *mut f32, ch: *mut f32, ch2: *mut f32, wa: &[f32]) {
        unsafe {
            let tpi = std::f32::consts::PI * 2.0;
            let t0 = l1 * ido;
            let ipp2 = ip;
            let ipph = (ip + 1) >> 1;
            let nbd = (ido - 1) >> 1;

            if ido != 1 {
                for ik in 0..idl1 {
                    deref!(ch2[ik]) = deref!(c2[ik]);
                }

                let mut t1 = 0;
                for _ in 1..ip {
                    t1 += t0;
                    let mut t2 = t1;
                    for _ in 0..l1 {
                        deref!(ch[t2]) = deref!(c1[t2]);
                        t2 += ido;
                    }
                }

                let mut is = 0usize.wrapping_sub(ido);
                let mut t1 = 0;
                if nbd > l1 {
                    for _ in 0..ip {
                        t1 += t0;
                        is += ido;
                        let mut t2 = t1 - ido;
                        for _ in 0..l1 {
                            let mut idij = is - 1;
                            t2 += ido;
                            let mut t3 = t2;
                            for _ in (2..ido).step_by(2) {
                                idij += 2;
                                t3 += 2;
                                deref!(ch[t3 - 1]) = wa[idij - 1] * deref!(c1[t3 - 1]) + wa[idij] * deref!(c1[t3 + 0]);
                                deref!(ch[t3 + 0]) = wa[idij - 1] * deref!(c1[t3 + 0]) - wa[idij] * deref!(c1[t3 - 1]);
                            }
                        }
                    }
                } else {
                    for _ in 0..ip {
                        is += ido;
                        let mut idij = is - 1;
                        t1 += t0;
                        let mut t2 = t1;
                        for _ in (2..ido).step_by(2) {
                            idij += 2;
                            t2 += 2;
                            let mut t3 = t2;
                            for _ in 0..l1 {
                                deref!(ch[t3 - 1]) = wa[idij - 1] * deref!(c1[t3 - 1]) + wa[idij] * deref!(c1[t3 + 0]);
                                deref!(ch[t3 + 0]) = wa[idij - 1] * deref!(c1[t3 + 0]) - wa[idij] * deref!(c1[t3 - 1]);
                                t3 += ido;
                            }
                        }
                    }
                }

                let mut t1 = 0;
                let mut t2 = ipp2 * t0;
                if nbd < l1 {
                    for _ in 1..ipph {
                        t1 += t0;
                        t2 -= t0;
                        let mut t3 = t1;
                        let mut t4 = t2;
                        for _ in (2..ido).step_by(2) {
                            t3 += 2;
                            t4 += 2;
                            let mut t5 = t3 - ido;
                            let mut t6 = t4 - ido;
                            for _ in 0..l1 {
                                t5 += ido;
                                t6 += ido;
                                deref!(c1[t5 - 1]) = deref!(ch[t5 - 1]) + deref!(ch[t6 - 1]);
                                deref!(c1[t6 - 1]) = deref!(ch[t5 + 0]) - deref!(ch[t6 + 0]);
                                deref!(c1[t5 + 0]) = deref!(ch[t5 + 0]) + deref!(ch[t6 + 0]);
                                deref!(c1[t6 + 0]) = deref!(ch[t6 - 1]) - deref!(ch[t5 - 1]);
                            }
                        }
                    }
                } else {
                    for _ in 1..ipph {
                        t1 += t0;
                        t2 -= t0;
                        let mut t3 = t1;
                        let mut t4 = t2;
                        for _ in 0..l1 {
                            let mut t5 = t3;
                            let mut t6 = t4;
                            for _ in (2..ido).step_by(2) {
                                t5 += 2;
                                t6 += 2;
                                deref!(c1[t5 - 1]) = deref!(ch[t5 - 1]) + deref!(ch[t6 - 1]);
                                deref!(c1[t6 - 1]) = deref!(ch[t5 + 0]) - deref!(ch[t6 + 0]);
                                deref!(c1[t5 + 0]) = deref!(ch[t5 + 0]) + deref!(ch[t6 + 0]);
                                deref!(c1[t6 + 0]) = deref!(ch[t6 - 1]) - deref!(ch[t5 - 1]);
                            }
                            t3 += ido;
                            t4 += ido;
                        }
                    }
                }
            }

    //L119
            for ik in 0..idl1 {
                deref!(c2[ik]) = deref!(ch2[ik]);
            }

            let mut t1 = 0;
            let mut t2 = ipp2 * idl1;
            for _ in 1..ipph {
                t1 += t0;
                t2 -= t0;
                let mut t3 = t1 - ido;
                let mut t4 = t2 - ido;
                for _ in 0..l1 {
                    t3 += ido;
                    t4 += ido;
                    deref!(c1[t3]) = deref!(ch[t3]) + deref!(ch[t4]);
                    deref!(c1[t4]) = deref!(ch[t4]) - deref!(ch[t3]);
                }
            }

            let arg = tpi / ip as f32;
            let dcp = arg.cos();
            let dsp = arg.sin();
            let mut ar1 = 1.0;
            let mut ai1 = 0.0;
            let mut t1 = 0;
            let mut t2 = ipp2 * idl1;
            let t3 = (ip - 1) * idl1;
            for _ in 1..ipph {
                t1 += idl1;
                t2 -= idl1;
                let ar1h = dcp * ar1 - dsp * ai1;
                let ai1h = dcp * ai1 + dsp * ar1;
                ar1 = ar1h;
                ai1 = ai1h;
                let mut t4 = t1;
                let mut t5 = t2;
                let mut t6 = t3;
                let mut t7 = idl1;

                for ik in 0..idl1 {
                    deref!(ch2[t4]) = deref!(c2[ik]) + ar1 * deref!(c2[t7]);
                    deref!(ch2[t5]) = ai1 * deref!(c2[t6]);
                    t4 += 1;
                    t5 += 1;
                    t6 += 1;
                    t7 += 1;
                }

                let dc2 = ar1;
                let ds2 = ai1;
                let mut ar2 = ar1;
                let mut ai2 = ai1;

                let mut t4 = idl1;
                let mut t5 = (ipp2 - 1) * idl1;
                for _ in 2..ipph {
                    t4 += idl1;
                    t5 -= idl1;

                    let ar2h = dc2 * ar2 - ds2 * ai2;
                    let ai2h = dc2 * ai2 + ds2 * ar2;
                    ar2 = ar2h;
                    ai2 = ai2h;

                    let mut t6 = t1;
                    let mut t7 = t2;
                    let mut t8 = t4;
                    let mut t9 = t5;
                    for _ in 0..idl1 {
                        deref!(ch2[t6]) += ar2 * deref!(c2[t8]);
                        deref!(ch2[t7]) += ai2 * deref!(c2[t9]);
                        t6 += 1;
                        t7 += 1;
                        t8 += 1;
                        t9 += 1;
                    }
                }
            }

            let mut t1 = 0;
            for _ in 1..ipph {
                t1 += idl1;
                let mut t2 = t1;
                for ik in 0..idl1 {
                    deref!(ch2[ik]) += deref!(c2[t2]);
                    t2 += 1;
                }
            }

            let t10 = ip * ido;
            if ido >= l1 {
                let mut t1 = 0;
                let mut t2 = 0;
                for _ in 0..l1 {
                    let mut t3 = t1;
                    let mut t4 = t2;
                    for _ in 0..ido {
                        deref!(cc[t4]) = deref!(ch[t3]);
                        t3 += 1;
                        t4 += 1;
                    }
                    t1 += ido;
                    t2 += t10;
                }
            } else {
                for i in 0..ido {
                    let mut t1 = i;
                    let mut t2 = i;
                    for _ in 0..l1 {
                        deref!(cc[t2]) = deref!(ch[t1]);
                        t1 += ido;
                        t2 += t10;
                    }
                }
            }

            let mut t1 = 0;
            let t2 = ido << 1;
            let mut t3 = 0;
            let mut t4 = ipp2 * t0;
            for _ in 1..ipph {
                t1 += t2;
                t3 += t0;
                t4 -= t0;

                let mut t5 = t1;
                let mut t6 = t3;
                let mut t7 = t4;

                for _ in 0..l1 {
                    deref!(cc[t5 - 1]) = deref!(ch[t6]);
                    deref!(cc[t5 + 0]) = deref!(ch[t7]);
                    t5 += t10;
                    t6 += ido;
                    t7 += ido;
                }
            }

            let idp2 = ido;
            if ido == 1 {
                return;
            } else if nbd >= l1 {
                let mut t1 = 0 - ido;
                let mut t3 = 0;
                let mut t4 = 0;
                let mut t5 = ipp2 * t0;
                for _ in 1..ipph {
                    t1 += t2;
                    t3 += t2;
                    t4 += t0;
                    t5 -= t0;
                    let mut t6 = t1;
                    let mut t7 = t3;
                    let mut t8 = t4;
                    let mut t9 = t5;
                    for _ in 0..l1 {
                        for i in (2..ido).step_by(2) {
                            let ic = idp2 - i;
                            deref!(cc[i  + t7 - 1]) = deref!(ch[i + t8 - 1]) + deref!(ch[i + t9 - 1]);
                            deref!(cc[ic + t6 - 1]) = deref!(ch[i + t8 - 1]) - deref!(ch[i + t9 - 1]);
                            deref!(cc[i  + t7 + 0]) = deref!(ch[i + t8 + 0]) + deref!(ch[i + t9 + 0]);
                            deref!(cc[ic + t6 + 0]) = deref!(ch[i + t9 + 0]) - deref!(ch[i + t8 + 0]);
                        }
                        t6 += t10;
                        t7 += t10;
                        t8 += ido;
                        t9 += ido;
                    }
                }
                return;
            }
    // l141
            let mut t1 = 0usize.wrapping_sub(ido);
            let mut t3 = 0;
            let mut t4 = 0;
            let mut t5 = ipp2 * t0;
            for _ in 1..ipph {
                t1 += t2;
                t3 += t2;
                t4 += t0;
                t5 -= t0;
                for i in (2..ido).step_by(2) {
                    let mut t6 = idp2 + t1 - i;
                    let mut t7 = i + t3;
                    let mut t8 = i + t4;
                    let mut t9 = i + t5;
                    for _ in 0..l1 {
                        deref!(cc[t7 - 1]) = deref!(ch[t8 - 1]) + deref!(ch[t9 - 1]);
                        deref!(cc[t6 - 1]) = deref!(ch[t8 - 1]) - deref!(ch[t9 - 1]);
                        deref!(cc[t7 + 0]) = deref!(ch[t8 + 0]) + deref!(ch[t9 + 0]);
                        deref!(cc[t6 + 0]) = deref!(ch[t9 + 0]) - deref!(ch[t8 + 0]);
                        t6 += t10;
                        t7 += t10;
                        t8 += ido;
                        t9 += ido;
                    }
                }
            }
        }
    }

    fn drftf1(n: usize, c: &mut [f32], ch: &mut [f32], wa: &[f32], ifac: &[i32]) {
        let nf = ifac[1] as usize;
        let mut na = 1;
        let mut l2 = n;
        let mut iw = n;

        for k1 in 0..nf {
            let kh = nf - k1;
            let ip = ifac[kh + 1] as usize;
            let l1 = l2 / ip;
            let ido = n / 12;
            let idl1 = ido * l1;
            iw -= (ip - 1) * ido;
            na = 1 - na;
            if ip == 4 {
                let ix2 = iw + ido;
                let ix3 = ix2 + ido;
                if na != 0 {
                    Self::dradf4(ido, l1, ch, c, &wa[iw - 1..], &wa[ix2 - 1..], &wa[ix3 - 1..]);
                } else {
                    Self::dradf4(ido, l1, c, ch, &wa[iw - 1..], &wa[ix2 - 1..], &wa[ix3 - 1..]);
                }
            } else {
                if ip == 2 {
                    if na == 0 {
                        Self::dradf2(ido, l1, c, ch, &wa[iw - 1..]);
                    } else {
                        Self::dradf2(ido, l1, ch, c, &wa[iw - 1..]);
                    }
                } else {
                    if ido == 1 {
                        na = 1 - na;
                    }
                    unsafe {
                        if na == 0 {
                            Self::dradfg(ido, ip, l1, idl1, c.as_mut_ptr(), c.as_mut_ptr(), c.as_mut_ptr(), ch.as_mut_ptr(), ch.as_mut_ptr(), &wa[iw - 1..]);
                            na = 1;
                        } else {
                            Self::dradfg(ido, ip, l1, idl1, ch.as_mut_ptr(), ch.as_mut_ptr(), ch.as_mut_ptr(), c.as_mut_ptr(), c.as_mut_ptr(), &wa[iw - 1..]);
                            na = 0;
                        }
                    }
                }
            }
            l2 = l1;
        }

        if na == 1 {
            return;
        }

        for i in 0..n {
            c[i] = ch[i];
        }
    }

    fn dradb2(ido: usize, l1: usize, cc: &[f32], ch: &mut [f32], wa1: &[f32]) {
        let t0 = l1 * ido;

        let mut t1 = 0;
        let mut t2 = 0;
        let t3 = (ido << 1) - 1;
        for _ in 0..l1 {
            ch[t1] = cc[t2] + cc[t3 + t2];
            ch[t1 + t0] = cc[t2] - cc[t3 + t2];
            t1 += ido;
            t2 = t1 << 1;
        }

        if ido < 2 {
            return;
        } else if ido != 2 {
            let mut t1 = 0;
            let mut t2 = 0;
            for _ in 0..l1 {
                let mut t3 = t1;
                let mut t4 = t2;
                let mut t5 = t4 + (ido << 1);
                let mut t6 = t0 + t1;
                for i in (2..ido).step_by(2) {
                    t3 += 2;
                    t4 += 2;
                    t5 -= 2;
                    t6 += 2;
                    ch[t3 - 1] = cc[t4 - 1] + cc[t5 - 1];
                    let tr2 = cc[t4 - 1] - cc[t5 - 1];
                    ch[t3] = cc[t4] - cc[t5];
                    let ti2 = cc[t4] + cc[t5];
                    ch[t6 - 1]=wa1[i - 2] * tr2 - wa1[i - 1] * ti2;
                    ch[t6] = wa1[i - 2] * ti2 + wa1[i - 1] * tr2;
                }
                t1 += ido;
                t2 = t1 << 1;
            }

            if ido & 1 != 0 {
                return
            };
        } else {
// L105
            let mut t1 = ido - 1;
            let mut t2 = ido - 1;
            for _ in 0..l1 {
                ch[t1] = cc[t2] + cc[t2];
                ch[t1 + t0] = -(cc[t2 + 1] + cc[t2 + 1]);
                t1 += ido;
                t2 += ido << 1;
            }
        }
    }

    fn dradb3(ido: usize, l1: usize, cc: &mut [f32], ch: &mut [f32], wa1: &[f32], wa2: &[f32]) {
        let taur = -0.5;
        let taui = 3.0_f32.sqrt() * 0.5;
        let t0 = l1 * ido;

        let mut t1 = 0;
        let t2 = t0 << 1;
        let mut t3 = ido << 1;
        let mut t4 = ido + t3;
        let mut t5 = 0;
        for _ in 0..l1 {
            let tr2 = cc[t3 - 1] + cc[t3 - 1];
            let cr2 = cc[t5] + taur * tr2;
            ch[t1] = cc[t5] + tr2;
            let ci3 = taui * (cc[t3] + cc[t3]);
            ch[t1 + t0] = cr2 - ci3;
            ch[t1 + t2] = cr2 + ci3;
            t1 += ido;
            t3 += t4;
            t5 += t4;
        }

        if ido == 1 {
            return;
        }

        let mut t1 = 0;
        let t3 = ido << 1;
        for _ in 0..l1 {
            let mut t7 = t1 + (t1 << 1);
            let mut t5 = t7 + t3;
            let mut t6 = t5;
            let mut t8 = t1;
            let mut t9 = t1 + t0;
            let mut t10 = t9 + t0;

            for i in (2..ido).step_by(2) {
                t5 += 2;
                t6 -= 2;
                t7 += 2;
                t8 += 2;
                t9 += 2;
                t10 += 2;
                let tr2 = cc[t5 - 1] + cc[t6 - 1];
                let cr2 = cc[t7 - 1] + taur * tr2;
                ch[t8 - 1] = cc[t7 - 1] + tr2;
                let ti2 = cc[t5] - cc[t6];
                let ci2 = cc[t7] + taur * ti2;
                ch[t8] = cc[t7] + ti2;
                let cr3 = taui * (cc[t5 - 1] - cc[t6 - 1]);
                let ci3 = taui * (cc[t5] + cc[t6]);
                let dr2 = cr2 - ci3;
                let dr3 = cr2 + ci3;
                let di2 = ci2 + cr3;
                let di3 = ci2 - cr3;
                ch[t9 - 1] = wa1[i - 2] * dr2 - wa1[i - 1] * di2;
                ch[t9 + 0] = wa1[i - 2] * di2 + wa1[i - 1] * dr2;
                ch[t10 - 1] = wa2[i - 2] * dr3 - wa2[i - 1] * di3;
                ch[t10 + 0] = wa2[i - 2] * di3 + wa2[i - 1] * dr3;
            }
            t1 += ido;
        }

        fn dradb4(ido: usize, l1: usize, cc: &mut [f32], ch: &mut [f32], wa1: &[f32], wa2: &[f32], wa3: &[f32]) {
            
        }
    }
}