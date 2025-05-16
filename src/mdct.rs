use std::slice::{from_raw_parts, from_raw_parts_mut};

macro_rules! rint {
    ($x:expr) => {
        $x.floor() + 0.5
    };
}

/// * This is for the modified DCT transform forward and backward
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MdctLookup {
    n: usize,
    log2n: i32,
    trig: Vec<f32>,
    bitrev: Vec<i32>,
    scale: f32,
}

const COS_PI3_8: f32 = 0.3826834323650897717284599840304; // (std::f32::consts::PI * 3.0 / 8.0).cos();
const COS_PI2_8: f32 = 0.70710678118654752440084436210485; // (std::f32::consts::PI * 2.0 / 8.0).cos();
const COS_PI1_8: f32 = 0.92387953251128675612818318939679; // (std::f32::consts::PI * 1.0 / 8.0).cos();

impl MdctLookup {
    /// * build lookups for trig functions; also pre-figure scaling and some window function algebra.
    pub fn new(n: usize) -> Self {
        let pi = std::f32::consts::PI;
        let n2 = n >> 1;
        let n4 = n >> 2;
        let n8 = n >> 3;
        let log2n = rint!((n as f32).ln() / 2.0_f32.ln()) as i32;
        let mut bitrev = vec![0; n4];
        let mut trig = vec![0.0f32; n + n4];

        let n_f = n as f32;
        let pi_div_n = pi / n_f;
        let pi_div_n2 = pi / (2.0 * n_f);
        for i in 0..n4 {
            let i_f = i as f32;
            let i2 = i_f * 2.0;
            let i4 = i_f * 4.0;
            trig[i * 2 + 0] =  (pi_div_n * i4).cos();
            trig[i * 2 + 1] = -(pi_div_n * i4).sin();
            trig[n2 + i * 2 + 0] = (pi_div_n2 * (i2 + 1.0)).cos();
            trig[n2 + i * 2 + 1] = (pi_div_n2 * (i2 + 1.0)).sin();
        }
        for i in 0..n8 {
            let i_f = i as f32;
            let i4 = i_f * 4.0;
            trig[n + i * 2 + 0] =  (pi_div_n * (i4 + 2.0)).cos() * 0.5;
            trig[n + i * 2 + 1] = -(pi_div_n * (i4 + 2.0)).sin() * 0.5;
        }

        let mask = (1 << (log2n - 1)) - 1;
        let msb = 1 << (log2n - 2);
        for i in 0..n8 {
            let mut acc = 0;
            let mut j = 0;
            let mut msb_rsh_j = msb >> j;
            while j < msb_rsh_j {
                if msb_rsh_j & i != 0 {
                    acc |= 1 << j;
                }
                j += 1;
                msb_rsh_j = msb >> j;
            }
            bitrev[i * 2 + 0] = ((!acc) & mask) - 1;
            bitrev[i * 2 + 1] = acc;
        }

        Self {
            n,
            log2n,
            trig,
            bitrev,
            scale: 4.0 / n as f32,
        }
    }

    /// * 8 point butterfly (in place, 4 register)
    pub fn butterfly_8(x: &mut [f32]) {
        let r0 = x[6] + x[2];
        let r1 = x[6] - x[2];
        let r2 = x[4] + x[0];
        let r3 = x[4] - x[0];

        x[6] = r0 + r2;
        x[4] = r0 - r2;

        let r0 = x[5] - x[1];
        let r2 = x[7] - x[3];
        x[0] = r1 + r0;
        x[2] = r1 - r0;

        let r0 = x[5] + x[1];
        let r1 = x[7] + x[3];
        x[3] = r2 + r3;
        x[1] = r2 - r3;
        x[7] = r1 + r0;
        x[5] = r1 - r0;
    }

    /// * 16 point butterfly (in place, 4 register)
    pub fn butterfly_16(x: &mut [f32]) {
        let r0 = x[1] - x[9];
        let r1 = x[0] - x[8];

        x[8]  += x[0];
        x[9]  += x[1];
        x[0]   = (r0 + r1) * COS_PI2_8;
        x[1]   = (r0 - r1) * COS_PI2_8;

        let r0 = x[3]  - x[11];
        let r1 = x[10] - x[2];
        x[10] += x[2];
        x[11] += x[3];
        x[2]   = r0;
        x[3]   = r1;

        let r0 = x[12] - x[4];
        let r1 = x[13] - x[5];
        x[12] += x[4];
        x[13] += x[5];
        x[4]   = (r0 - r1) * COS_PI2_8;
        x[5]   = (r0 + r1) * COS_PI2_8;

        let r0 = x[14] - x[6];
        let r1 = x[15] - x[7];
        x[14] += x[6];
        x[15] += x[7];
        x[6]  = r0;
        x[7]  = r1;

        Self::butterfly_8(x);
        Self::butterfly_8(&mut x[8..]);
    }

    /// *  32 point butterfly (in place, 4 register)
    pub fn butterfly_32(x: &mut [f32]) {
        let r0 = x[30] - x[14];
        let r1 = x[31] - x[15];

        x[30] +=         x[14];
        x[31] +=         x[15];
        x[14]  =         r0;
        x[15]  =         r1;

        let r0 = x[28] - x[12];
        let r1 = x[29] - x[13];
        x[28] +=         x[12];
        x[29] +=         x[13];
        x[12]  = r0 * COS_PI1_8  -  r1 * COS_PI3_8;
        x[13]  = r0 * COS_PI3_8  +  r1 * COS_PI1_8;

        let r0 = x[26] - x[10];
        let r1 = x[27] - x[11];
        x[26] +=         x[10];
        x[27] +=         x[11];
        x[10]  = ( r0  - r1 ) * COS_PI2_8;
        x[11]  = ( r0  + r1 ) * COS_PI2_8;

        let r0 = x[24] - x[8];
        let r1 = x[25] - x[9];
        x[24] += x[8];
        x[25] += x[9];
        x[8]   = r0 * COS_PI3_8  -  r1 * COS_PI1_8;
        x[9]   = r1 * COS_PI3_8  +  r0 * COS_PI1_8;

        let r0 = x[22] - x[6];
        let r1 = x[7]  - x[23];
        x[22] += x[6];
        x[23] += x[7];
        x[6]   = r1;
        x[7]   = r0;

        let r0 = x[4]  - x[20];
        let r1 = x[5]  - x[21];
        x[20] += x[4];
        x[21] += x[5];
        x[4]   = r1 * COS_PI1_8  +  r0 * COS_PI3_8;
        x[5]   = r1 * COS_PI3_8  -  r0 * COS_PI1_8;

        let r0 = x[2]  - x[18];
        let r1 = x[3]  - x[19];
        x[18] += x[2];
        x[19] += x[3];
        x[2]   = ( r1  + r0 ) * COS_PI2_8;
        x[3]   = ( r1  - r0 ) * COS_PI2_8;

        let r0 = x[0]  - x[16];
        let r1 = x[1]  - x[17];
        x[16] += x[0];
        x[17] += x[1];
        x[0]   = r1 * COS_PI3_8  +  r0 * COS_PI1_8;
        x[1]   = r1 * COS_PI1_8  -  r0 * COS_PI3_8;

        Self::butterfly_16(x);
        Self::butterfly_16(&mut x[16..]);
    }

    /// * N point first stage butterfly (in place, 2 register)
    pub fn butterfly_first(mut t: &[f32], x: &mut [f32], points: usize) {
        let x = x.as_mut_ptr();
        let mut x1 = unsafe {x.add((points >> 0) - 8)};
        let mut x2 = unsafe {x.add((points >> 1) - 8)};
        loop {
            unsafe {
                let x1 = from_raw_parts_mut(x1, 8);
                let x2 = from_raw_parts_mut(x2, 8);

                let r0   = x1[6]      -  x2[6];
                let r1   = x1[7]      -  x2[7];
                x1[6]  += x2[6];
                x1[7]  += x2[7];
                x2[6]   = r1 * t[1]  +  r0 * t[0];
                x2[7]   = r1 * t[0]  -  r0 * t[1];

                let r0  = x1[4]      -  x2[4];
                let r1  = x1[5]      -  x2[5];
                x1[4]  += x2[4];
                x1[5]  += x2[5];
                x2[4]   = r1 * t[5]  +  r0 * t[4];
                x2[5]   = r1 * t[4]  -  r0 * t[5];

                let r0  = x1[2]      -  x2[2];
                let r1  = x1[3]      -  x2[3];
                x1[2]  += x2[2];
                x1[3]  += x2[3];
                x2[2]   = r1 * t[9]  +  r0 * t[8];
                x2[3]   = r1 * t[8]  -  r0 * t[9];

                let r0  = x1[0]      -  x2[0];
                let r1  = x1[1]      -  x2[1];
                x1[0]  += x2[0];
                x1[1]  += x2[1];
                x2[0]   = r1 * t[13] +  r0 * t[12];
                x2[1]   = r1 * t[12] -  r0 * t[13];
            }

            x1 = unsafe {x1.sub(8)};
            x2 = unsafe {x2.sub(8)};
            t = &t[16..];
            if x2 < x {
                break;
            }
        }
    }

    /// * N/stage point generic N stage butterfly (in place, 2 register)
    pub fn butterfly_generic(mut t: &[f32], x: &mut [f32], points: usize, trigint: usize) {
        let x = x.as_mut_ptr();
        let mut x1 = unsafe {x.add((points >> 0) - 8)};
        let mut x2 = unsafe {x.add((points >> 1) - 8)};
        loop {
            unsafe {
                let x1 = from_raw_parts_mut(x1, 8);
                let x2 = from_raw_parts_mut(x2, 8);

                let r0  = x1[6]      -  x2[6];
                let r1  = x1[7]      -  x2[7];
                x1[6]  += x2[6];
                x1[7]  += x2[7];
                x2[6]   = r1 * t[1]  +  r0 * t[0];
                x2[7]   = r1 * t[0]  -  r0 * t[1];

                t = &t[trigint..];

                let r0  = x1[4]      -  x2[4];
                let r1  = x1[5]      -  x2[5];
                x1[4]  += x2[4];
                x1[5]  += x2[5];
                x2[4]   = r1 * t[1]  +  r0 * t[0];
                x2[5]   = r1 * t[0]  -  r0 * t[1];

                t = &t[trigint..];

                let r0  = x1[2]      -  x2[2];
                let r1  = x1[3]      -  x2[3];
                x1[2]  += x2[2];
                x1[3]  += x2[3];
                x2[2]   = r1 * t[1]  +  r0 * t[0];
                x2[3]   = r1 * t[0]  -  r0 * t[1];

                t = &t[trigint..];

                let r0  = x1[0]      -  x2[0];
                let r1  = x1[1]      -  x2[1];
                x1[0]  += x2[0];
                x1[1]  += x2[1];
                x2[0]   = r1 * t[1]  +  r0 * t[0];
                x2[1]   = r1 * t[0]  -  r0 * t[1];

                t = &t[trigint..];
            }

            x1 = unsafe {x1.sub(8)};
            x2 = unsafe {x2.sub(8)};
            if x2 < x {
                break;
            }
        }
    }

    pub fn butterflies(&self, x: &mut [f32], points: usize) {
        let t = &self.trig;
        let mut stages = self.log2n - 5;

        stages -= 1;
        if stages > 0 {
            Self::butterfly_first(t, x, points);
        }

        let mut i = 1;
        loop { // for(i=1;--stages>0;i++)
            stages -= 1;
            if stages <= 0 {
                break;
            }

            let cur_stage_points = points >> i;
            for j in 0..(1 << i) {
                // mdct_butterfly_generic(T,x+(points>>i)*j,points>>i,4<<i);
                Self::butterfly_generic(t, &mut x[cur_stage_points * j..], cur_stage_points, 4 << i);
            }

            i += 1;
        }

        for j in (0..points).step_by(32) { // for(j=0;j<points;j+=32)
            Self::butterfly_32(&mut x[j..]);
        }
    }

    pub fn bitreverse(&self, x: &mut [f32]) {
        let n = self.n;
        let mut bit = &self.bitrev[..];
        let mut x = x.as_mut_ptr();
        let mut w0 = x;
        let mut w1 = unsafe {w0.add(n >> 1)};
        x = w1;
        let mut t = &self.trig[n..];

        loop {
            unsafe {
                w1 = w1.sub(4);
                let w0 = from_raw_parts_mut(w0, 4);
                let w1 = from_raw_parts_mut(w1.add(4), 4);

                let x0 = from_raw_parts(x.add(bit[0] as usize), 2);
                let x1 = from_raw_parts(x.add(bit[1] as usize), 2);

                let r0 = x0[1]  - x1[1];
                let r1 = x0[0]  + x1[0];
                let r2 = r1     * t[0]   + r0 * t[1];
                let r3 = r1     * t[1]   - r0 * t[0];

                let r0 = (x0[1] + x1[1]) * 0.5;
                let r1 = (x0[0] - x1[0]) * 0.5;

                w0[0]  = r0     + r2;
                w1[2]  = r0     - r2;
                w0[1]  = r1     + r3;
                w1[3]  = r3     - r1;

                let x0 = from_raw_parts(x.add(bit[2] as usize), 2);
                let x1 = from_raw_parts(x.add(bit[3] as usize), 2);

                let r0 = x0[1]  - x1[1];
                let r1 = x0[0]  + x1[0];
                let r2 = r1     * t[2]   + r0 * t[3];
                let r3 = r1     * t[3]   - r0 * t[2];

                let r0 = (x0[1] + x1[1]) * 0.5;
                let r1 = (x0[0] - x1[0]) * 0.5;

                w0[2]  = r0     + r2;
                w1[0]  = r0     - r2;
                w0[3]  = r1     + r3;
                w1[1]  = r3     - r1;
            }

            t     = &t[4..];
            bit   = &bit[4..];
            w0    = unsafe {w0.add(4)};

            if w0 >= w1 {
                break;
            }
        }
    }

    pub fn backward(&self, in_: &[f32], out: &mut [f32]) {
        let outlen = out.len();
        let n = self.n;
        let n2 = n >> 1;
        let n4 = n >> 2;
        let in_ = in_.as_ptr();
        let out = out.as_mut_ptr();

        // rotate

        let mut ix = unsafe {in_.add(n2).sub(7)};
        let mut ox = unsafe {out.add(n2 + n4)};
        let mut t = &self.trig[n4..];

        loop {
            ox = unsafe {ox.sub(4)};
            unsafe {
                let ix = from_raw_parts(ix, 8);
                let ox = from_raw_parts_mut(ox, 4);

                ox[0] = -ix[2] * t[3] - ix[0] * t[2];
                ox[1] =  ix[0] * t[3] - ix[2] * t[2];
                ox[2] = -ix[6] * t[1] - ix[4] * t[0];
                ox[3] =  ix[4] * t[1] - ix[6] * t[0];
            }
            ix = unsafe {ix.sub(8)};
            t = &t[4..];
            if ix < in_ {
                break;
            }
        }

        let mut ix = unsafe {in_.add(n2).sub(8)};
        let mut ox = unsafe {out.add(n2 + n4)};
        let mut t = unsafe {self.trig.as_ptr().add(n4)};

        loop {
            unsafe {
                t = t.sub(4);
                let t = from_raw_parts(t, 4);
                let ix = from_raw_parts(ix, 8);
                let ox = from_raw_parts_mut(ox, 4);

                ox[0] = ix[4] * t[3] + ix[6] * t[2];
                ox[1] = ix[4] * t[2] - ix[6] * t[3];
                ox[2] = ix[0] * t[1] + ix[2] * t[0];
                ox[3] = ix[0] * t[0] - ix[2] * t[1];
            }
            ix = unsafe {ix.sub(8)};
            ox = unsafe {ox.add(4)};
            if ix < in_ {
                break;
            }
        }

        let out = unsafe {from_raw_parts_mut(out, outlen)};

        self.butterflies(&mut out[n2..], n2);
        self.bitreverse(out);

        // roatate + window

        let out = out.as_mut_ptr();
        let mut ox1 = unsafe {out.add(n2 + n4)};
        let mut ox2 = unsafe {from_raw_parts_mut(ox1, outlen - n2 - n4)};
        let mut ix = out;
        let mut t = &self.trig[n2..];

        loop {
            unsafe {
                ox1 = ox1.sub(4);
                let ix = from_raw_parts(ix, 8);
                let ox1 = from_raw_parts_mut(ox1, 4);

                ox1[3] =  ix[0] * t[1] - ix[1] * t[0];
                ox2[0] = -ix[0] * t[0] + ix[1] * t[1];
                ox1[2] =  ix[2] * t[3] - ix[3] * t[2];
                ox2[1] = -ix[2] * t[2] + ix[3] * t[3];
                ox1[1] =  ix[4] * t[5] - ix[5] * t[4];
                ox2[2] = -ix[4] * t[4] + ix[5] * t[5];
                ox1[0] =  ix[6] * t[7] - ix[7] * t[6];
                ox2[3] = -ix[6] * t[6] + ix[7] * t[7];

            }
            ox2 = &mut ox2[4..];
            ix = unsafe {ix.add(8)};
            t = &t[8..];
            if ix >= ox1 {
                break;
            }
        }

        let mut ix = unsafe {out.add(n2 + n4)};
        let mut ox1 = unsafe {out.add(n4)};
        let mut ox2 = ox1;

        loop {
            unsafe {
                ox1 = ox1.sub(4);
                ix = ix.sub(4);

                let ix = from_raw_parts(ix, 4);
                let ox1 = from_raw_parts_mut(ox1, 4);
                let ox2 = from_raw_parts_mut(ox2, 4);

                ox1[3] = ix[3]; ox2[0] = -ox1[3];
                ox1[2] = ix[2]; ox2[1] = -ox1[2];
                ox1[1] = ix[1]; ox2[2] = -ox1[1];
                ox1[0] = ix[0]; ox2[3] = -ox1[0];
            }

            ox2 = unsafe{ox2.add(4)};
            if ox2 >= ix {
                break;
            }
        }

        let mut ix = unsafe {out.add(n2 + n4)};
        let mut ox1 = unsafe {out.add(n2 + n4)};
        let ox2 = unsafe {out.add(n2)};
        loop {
            unsafe {
                ox1 = ox1.sub(4);
                let ix = from_raw_parts(ix, 4);
                let ox1 = from_raw_parts_mut(ox1, 4);
                ox1[0]= ix[3];
                ox1[1]= ix[2];
                ox1[2]= ix[1];
                ox1[3]= ix[0];
            }
            ix = unsafe {ix.add(4)};
            if ox1 <= ox2 {
                break;
            }
        }
    }

    pub fn forward(&self, in_: &[f32], out: &mut [f32]) {
        let n = self.n;
        let n2 = n >> 1;
        let n4 = n >> 2;
        let n8 = n >> 3;
        let mut w = vec![0.0_f32; n]; // forward needs working space
        let w2 = &mut w[n2..];
        let in_ = in_.as_ptr();

        // rotate

        // window + rotate + step 1

        let mut x0 = unsafe {in_.add(n2 + n4)};
        let mut x1 = unsafe {x0.add(1)};
        let mut t = unsafe {self.trig.as_ptr().add(n2)};
        let mut i = 0;

        while i < n8 {
            t = unsafe {t.sub(2)};
            x0 = unsafe {x0.sub(4)};
            unsafe {
                let t = from_raw_parts(t, 2);
                let x0 = from_raw_parts(x0, 4);
                let x1 = from_raw_parts(x1, 4);
                let r0 = x0[2] + x1[0];
                let r1 = x0[0] + x1[2];
                w2[i + 0] = r1 * t[1] + r0 * t[0];
                w2[i + 1] = r1 * t[0] - r0 * t[1];
            }
            x1 = unsafe {x1.add(4)};
            i += 2;
        }

        x1 = unsafe {in_.add(1)};

        while i < n2 - n8 {
            t = unsafe {t.sub(2)};
            x0 = unsafe {x0.sub(4)};
            unsafe {
                let t = from_raw_parts(t, 2);
                let x0 = from_raw_parts(x0, 4);
                let x1 = from_raw_parts(x1, 4);
                let r0 = x0[2] - x1[0];
                let r1 = x0[0] - x1[2];
                w2[i + 0] = r1 * t[1] + r0 * t[0];
                w2[i + 1] = r1 * t[0] - r0 * t[1];
            }
            x1 = unsafe {x1.add(4)};
            i += 2;
        }

        x0 = unsafe {in_.add(n)};

        while i < n2 {
            t = unsafe {t.sub(2)};
            x0 = unsafe {x0.sub(4)};
            unsafe {
                let t = from_raw_parts(t, 2);
                let x0 = from_raw_parts(x0, 4);
                let x1 = from_raw_parts(x1, 4);
                let r0 = -x0[2] - x1[0];
                let r1 = -x0[0] - x1[2];
                w2[i + 0] = r1 * t[1] + r0 * t[0];
                w2[i + 1] = r1 * t[0] - r0 * t[1];
            }
            x1 = unsafe {x1.add(4)};
            i += 2;
        }

        self.butterflies(&mut w[n2..], n2);
        self.bitreverse(&mut w);

        // roatate + window

        let mut t = &self.trig[n2..];
        let mut x0 = out[n2..].as_mut_ptr();
        let mut w = &w[..];

        for i in 0..n4 {
            x0 = unsafe {x0.sub(1)};
            out[i] = (w[0] * t[0] + w[1] * t[1]) * self.scale;
            unsafe{*x0 = (w[0] * t[1] - w[1] * t[0]) * self.scale};
            w = &w[2..];
            t = &t[2..];
        }
    }
}
