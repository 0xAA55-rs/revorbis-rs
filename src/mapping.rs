use std::{
    fmt::{self, Debug, Formatter},
    io::{self, Write},
};

use crate::*;
use headers::{VorbisSetupHeader, VorbisIdentificationHeader};
use copiablebuf::CopiableBuffer;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VorbisMapping {
    /// Mapping type
    pub mapping_type: i32,

    /// <= 16
    pub submaps: i32,

    /// up to 256 channels in a Vorbis stream
    pub chmuxlist: CopiableBuffer<i32, 256>,

    /// [mux] submap to floors
    pub floorsubmap: CopiableBuffer<i32, 16>,

    /// [mux] submap to residue
    pub residuesubmap: CopiableBuffer<i32, 16>,

    pub coupling_steps: i32,
    pub coupling_mag: CopiableBuffer<i32, 256>,
    pub coupling_ang: CopiableBuffer<i32, 256>,
}

impl VorbisMapping {
    pub fn load(bitreader: &mut BitReader, vorbis_info: &VorbisSetupHeader, ident_header: &VorbisIdentificationHeader) -> Result<Self, io::Error> {
        let mapping_type = read_bits!(bitreader, 16);

        if mapping_type != 0 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Invalid mapping type {mapping_type}")))
        }

        let channels = ident_header.channels as i32;
        let floors = vorbis_info.floors.len() as i32;
        let residues = vorbis_info.residues.len() as i32;
        let submaps = if read_bits!(bitreader, 1) != 0 {
            let submaps = read_bits!(bitreader, 4).wrapping_add(1);
            if submaps == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No submaps.".to_string()));
            }
            submaps
        } else {
            1
        };
        let coupling_steps = if read_bits!(bitreader, 1) != 0 {
            let coupling_steps = read_bits!(bitreader, 8).wrapping_add(1);
            if coupling_steps == 0 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, "No coupling steps.".to_string()));
            }
            coupling_steps
        } else {
            0
        };
        let mut ret = Self {
            submaps,
            coupling_steps,
            ..Default::default()
        };

        let submaps = submaps as usize;
        let channels = channels as usize;
        let coupling_steps = coupling_steps as usize;

        ret.coupling_mag.resize(coupling_steps, 0);
        ret.coupling_ang.resize(coupling_steps, 0);
        for i in 0..coupling_steps {
            let test_m = read_bits!(bitreader, ilog!(channels - 1));
            let test_a = read_bits!(bitreader, ilog!(channels - 1));
            ret.coupling_mag[i] = test_m;
            ret.coupling_ang[i] = test_a;
            if test_m == test_a
            || test_m >= channels as i32
            || test_a >= channels as i32 {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Bad values for test_m = {test_m}, test_a = {test_a}, channels = {channels}")));
            }
        }

        let reserved = read_bits!(bitreader, 2);
        if reserved != 0 {
            return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Reserved value is {reserved}")));
        }

        if submaps > 1 {
            ret.chmuxlist.resize(channels, 0);
            for i in 0..channels {
                let chmux = read_bits!(bitreader, 4);
                if chmux >= submaps as i32 {
                    return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("Chmux {chmux} >= submaps {submaps}")));
                }
                ret.chmuxlist[i] = chmux;
            }
        }
        ret.floorsubmap.resize(submaps, 0);
        ret.residuesubmap.resize(submaps, 0);
        for i in 0..submaps {
            let _unused_time_submap = read_bits!(bitreader, 8);
            let floorsubmap = read_bits!(bitreader, 8);
            if floorsubmap >= floors {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("floorsubmap {floorsubmap} >= floors {floors}")));
            }
            ret.floorsubmap[i] = floorsubmap;
            let residuesubmap = read_bits!(bitreader, 8);
            if residuesubmap >= residues {
                return_Err!(io::Error::new(io::ErrorKind::InvalidData, format!("residuesubmap {residuesubmap} >= residues {residues}")));
            }
            ret.residuesubmap[i] = residuesubmap;
        }
        Ok(ret)
    }

    /// * Pack to the bitstream
    pub fn pack<W>(&self, bitwriter: &mut BitWriter<W>, channels: i32) -> Result<usize, io::Error>
    where
        W: Write {
        let begin_bits = bitwriter.total_bits;

        write_bits!(bitwriter, self.mapping_type, 16);
        if self.submaps > 1 {
            write_bits!(bitwriter, 1, 1);
            write_bits!(bitwriter, self.submaps.wrapping_sub(1), 4);
        } else {
            write_bits!(bitwriter, 0, 1);
        }

        if self.coupling_steps > 0 {
            write_bits!(bitwriter, 1, 1);
            write_bits!(bitwriter, self.coupling_steps.wrapping_sub(1), 8);
            for i in 0..self.coupling_steps as usize {
                write_bits!(bitwriter, self.coupling_mag[i], ilog!(channels - 1));
                write_bits!(bitwriter, self.coupling_ang[i], ilog!(channels - 1));
            }
        } else {
            write_bits!(bitwriter, 0, 1);
        }

        write_bits!(bitwriter, 0, 2);

        if self.submaps > 1 {
            for i in 0..channels as usize {
                write_bits!(bitwriter, self.chmuxlist[i], 4);
            }
        }
        for i in 0..self.submaps as usize {
            write_bits!(bitwriter, 0, 8); // time submap unused
            write_bits!(bitwriter, self.floorsubmap[i], 8);
            write_bits!(bitwriter, self.residuesubmap[i], 8);
        }

        Ok(bitwriter.total_bits - begin_bits)
    }
}

impl Debug for VorbisMapping {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisMapping")
        .field("mapping_type", &self.mapping_type)
        .field("submaps", &self.submaps)
        .field("chmuxlist", &format_args!("[{}]", format_array!(self.chmuxlist)))
        .field("floorsubmap", &format_args!("[{}]", format_array!(self.floorsubmap)))
        .field("residuesubmap", &format_args!("[{}]", format_array!(self.residuesubmap)))
        .field("coupling_steps", &self.coupling_steps)
        .field("coupling_mag", &format_args!("[{}]", format_array!(self.coupling_mag)))
        .field("coupling_ang", &format_args!("[{}]", format_array!(self.coupling_ang)))
        .finish()
    }
}

impl Default for VorbisMapping {
    fn default() -> Self {
        Self {
            mapping_type: 0,
            submaps: 0,
            chmuxlist: CopiableBuffer::default(),
            floorsubmap: CopiableBuffer::default(),
            residuesubmap: CopiableBuffer::default(),
            coupling_steps: 0,
            coupling_mag: CopiableBuffer::default(),
            coupling_ang: CopiableBuffer::default(),
        }
    }
}
