#![allow(dead_code)]
use crate::*;
use headers::VorbisMode;
use mapping::VorbisMapping;
use copiablebuf::CopiableBuffer;

pub const MODE_TEMPLATE: [VorbisMode; 2] = [
	VorbisMode {
		block_flag: false,
		window_type: 0,
		transform_type: 0,
		mapping: 0,
	},
	VorbisMode {
		block_flag: true,
		window_type: 0,
		transform_type: 0,
		mapping: 1,
	},
];

pub static MAP_NOMINAL: [VorbisMapping; 2] = [
	VorbisMapping {
		mapping_type: 0,
		submaps: 1,
		chmuxlist: CopiableBuffer::from_fixed_array([0, 0]),
		floorsubmap: CopiableBuffer::from_fixed_array([0]),
		residuesubmap: CopiableBuffer::from_fixed_array([0]),
		coupling_steps: 1,
		coupling_mag: CopiableBuffer::from_fixed_array([0]),
		coupling_ang: CopiableBuffer::from_fixed_array([1]),
	},
	VorbisMapping{
		mapping_type: 0,
		submaps: 1,
		chmuxlist: CopiableBuffer::from_fixed_array([0, 0]),
		floorsubmap: CopiableBuffer::from_fixed_array([1]),
		residuesubmap: CopiableBuffer::from_fixed_array([1]),
		coupling_steps: 1,
		coupling_mag: CopiableBuffer::from_fixed_array([0]),
		coupling_ang: CopiableBuffer::from_fixed_array([1]),
	},
];
