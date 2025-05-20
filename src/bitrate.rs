#![allow(dead_code)]
use std::{
    cmp::{min, max},
    fmt::Debug,
    io,
    mem,
    rc::Rc,
    cell::RefCell,
};

use crate::*;
use codec::VorbisInfo;
use blocks::VorbisBlock;

#[derive(Debug, Clone)]
pub struct VorbisBitrateManagerState {
    pub managed: bool,

    pub avg_reservoir: usize,
    pub minmax_reservoir: usize,
    pub avg_bitsper: i32,
    pub min_bitsper: i32,
    pub max_bitsper: i32,

    pub short_per_long: i32,
    pub avgfloat: f64,

    pub vorbis_block: Option<Rc<RefCell<VorbisBlock>>>,
    pub choice: i32,
}

impl VorbisBitrateManagerState {
    pub fn new(vorbis_info: &VorbisInfo) -> Self {
        let codec_setup = &vorbis_info.codec_setup;
        let manager_info = &codec_setup.bitrate_manager_info;

        if manager_info.reservoir_bits > 0 {
            let ratesamples = vorbis_info.sample_rate as f32;
            let halfsamples = (codec_setup.block_size[0] >> 1) as f32;
            let desired_fill = (manager_info.reservoir_bits as f64 * manager_info.reservoir_bias) as usize;
            Self {
                managed: true,
                short_per_long: codec_setup.block_size[1] / codec_setup.block_size[0],
                avg_bitsper: rint!(1.0 * manager_info.avg_rate as f32 * halfsamples / ratesamples),
                min_bitsper: rint!(1.0 * manager_info.min_rate as f32 * halfsamples / ratesamples),
                max_bitsper: rint!(1.0 * manager_info.max_rate as f32 * halfsamples / ratesamples),
                avgfloat: (PACKETBLOBS / 2) as f64,
                minmax_reservoir: desired_fill,
                avg_reservoir: desired_fill,
                vorbis_block: None,
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }

    /// Finish taking in the block we just processed
    pub fn add_block(&mut self, block: Rc<RefCell<VorbisBlock>>) -> io::Result<()> {
        let vb = block.borrow_mut();
        let vbi = &vb.internal.as_ref().expect("The block should be in encoding mode");
        let vd = &vb.vorbis_dsp_state;
        let b = &vd.backend_state;
        let vi = &vd.vorbis_info;
        let ci = &vi.codec_setup;
        let bi = &ci.bitrate_manager_info;

        let mut choice = rint!(self.avgfloat);
        let mut this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
        let min_target_bits = if vb.W != 0 {
            self.min_bitsper * self.short_per_long
        } else {
            self.min_bitsper
        } as usize;
        let max_target_bits = if vb.W != 0 {
            self.max_bitsper * self.short_per_long
        } else {
            self.max_bitsper
        } as usize;
        let samples = ci.block_size[vb.W as usize] >> 1;
        let desired_fill = (bi.reservoir_bits as f64 * bi.reservoir_bias) as usize;
        if !b.is_bitrate_managed() {
            /* not a bitrate managed stream, but for API simplicity, we'll
               buffer the packet to keep the code path clean */

            if self.vorbis_block.is_some() {
                // one has been submitted without being claimed
                panic!("A block has been submitted without being claimed");
            }
            self.vorbis_block = Some(block.clone());
            return Ok(())
        }

        self.vorbis_block = Some(block.clone());

        // look ahead for avg floater
        if self.avg_bitsper > 0 {
            let avg_target_bits = if vb.W != 0 {
                self.avg_bitsper * self.short_per_long
            } else {
                self.avg_bitsper
            } as usize;

            /* choosing a new floater:
               if we're over target, we slew down
               if we're under target, we slew up

               choose slew as follows: look through packetblobs of this frame
               and set slew as the first in the appropriate direction that
               gives us the slew we want.  This may mean no slew if delta is
               already favorable.

               Then limit slew to slew max */

            if self.avg_reservoir + (this_bits - avg_target_bits) > desired_fill {
                while choice > 0 && this_bits > avg_target_bits &&
                    self.avg_reservoir + (this_bits - avg_target_bits) > desired_fill {
                    choice -= 1;
                    this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
                }
            } else if self.avg_reservoir + (this_bits - avg_target_bits) < desired_fill {
                while choice + 1 > PACKETBLOBS as i32 && this_bits < avg_target_bits &&
                    self.avg_reservoir + (this_bits - avg_target_bits) < desired_fill {
                    choice += 1;
                    this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
                }
            }

            let slewlimit = 15.0 / bi.slew_damp;
            let slew = rint!(choice as f64 - self.avgfloat) as f64 / samples as f64 * vi.sample_rate as f64;
            let slew = slew.clamp(-slewlimit, slewlimit);
            self.avgfloat += slew / vi.sample_rate as f64 * samples as f64;
            choice = rint!(self.avgfloat);
            this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
        }

        // enforce min(if used) on the current floater (if used)
        if self.min_bitsper > 0 {
            // do we need to force the bitrate up?
            if this_bits < min_target_bits {
                while self.minmax_reservoir < min_target_bits - this_bits {
                    choice += 1;
                    if choice >= PACKETBLOBS as i32 {
                        break;
                    }
                    this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
                }
            }
        }

        // enforce max (if used) on the current floater (if used)
        if self.max_bitsper > 0 {
            // do we need to force the bitrate down?
            if this_bits > min_target_bits {
                while self.minmax_reservoir + (this_bits - max_target_bits) > bi.reservoir_bits {
                    choice -= 1;
                    if choice < 0 {
                        break;
                    }
                    this_bits = vbi.packetblob[choice as usize].borrow().get_total_bytes() * 8;
                }
            }
        }

        /* Choice of packetblobs now made based on floater, and min/max
           requirements. Now boundary check extreme choices */

        if choice < 0 {
            /* choosing a smaller packetblob is insufficient to trim bitrate.
               frame will need to be truncated */
            let maxsize = (max_target_bits + (bi.reservoir_bits - self.minmax_reservoir)) / 8;
            choice = 0;
            self.choice = 0;

            let mut chosen_packetblob = vbi.packetblob[choice as usize].borrow_mut();
            if chosen_packetblob.get_total_bytes() > maxsize {
                chosen_packetblob.write_trunc(maxsize * 8)?;
                this_bits = chosen_packetblob.get_total_bytes() * 8;
            }
        } else {
            let mut minsize = (min_target_bits - self.minmax_reservoir + 7) / 8;
            choice = max(choice, PACKETBLOBS as i32 - 1);

            self.choice = choice;

            // prop up bitrate according to demand. pad this frame out with zeroes
            let mut chosen_packetblob = vbi.packetblob[choice as usize].borrow_mut();
            minsize -= chosen_packetblob.get_total_bytes();
            write_slice!(chosen_packetblob, &vec![0u8; minsize]);
            this_bits = chosen_packetblob.get_total_bytes() * 8;
        }

        /* now we have the final packet and the final packet size.  Update statistics */
        /* min and max reservoir */
        if self.min_bitsper > 0 || self.max_bitsper > 0 {
            if max_target_bits > 0 && this_bits > max_target_bits {
                self.minmax_reservoir += this_bits - max_target_bits;
            } else if min_target_bits > 0 && this_bits < min_target_bits {
                self.minmax_reservoir += this_bits - min_target_bits;
            } else {
                // inbetween; we want to take reservoir toward but not past desired_fill
                if self.minmax_reservoir > desired_fill {
                    if max_target_bits > 0 { // logical bulletproofing against initialization state
                        self.minmax_reservoir += this_bits - max_target_bits;
                        self.minmax_reservoir = max(self.minmax_reservoir, desired_fill);
                    } else {
                        self.minmax_reservoir = desired_fill;
                    }
                } else {
                    if min_target_bits > 0 {
                        self.minmax_reservoir += this_bits - min_target_bits;
                        self.minmax_reservoir = min(self.minmax_reservoir, desired_fill);
                    } else {
                        self.minmax_reservoir = desired_fill;
                    }
                }
            }
        }

        // avg reservoir
        if self.avg_bitsper > 0 {
            self.avg_reservoir += this_bits - if vb.W != 0 {
                self.avg_bitsper * self.short_per_long
            } else {
                self.avg_bitsper
            } as usize;
        }

        Ok(())
    }
}

impl Default for VorbisBitrateManagerState {
    fn default() -> Self {
        use std::ptr::{write, addr_of_mut};
        let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
        unsafe {
            let ptr = ret_z.as_mut_ptr();
            write(addr_of_mut!((*ptr).vorbis_block), None);
            ret_z.assume_init()
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct VorbisBitrateManagerInfo {
    pub avg_rate: i32,
    pub min_rate: i32,
    pub max_rate: i32,
    pub reservoir_bits: usize,
    pub reservoir_bias: f64,

    pub slew_damp: f64,
}

