//! Support for the GGML file format.

use crate::Result;
use byteorder::{LittleEndian, ReadBytesExt};
use half::f16;

// Default to QK_K 256 rather than 64.
pub const QK_K: usize = 256;
pub const K_SCALE_SIZE: usize = 12;

pub const QK4_0: usize = 32;
pub const QK4_1: usize = 32;
pub const QK5_0: usize = 32;
pub const QK5_1: usize = 32;
pub const QK8_0: usize = 32;
pub const QK8_1: usize = 32;

#[repr(C)]
struct BlockQ4_0 {
    d: f16,
    qs: [u8; QK4_0 / 2],
}
// Hacky static_assert
const _: [u8; 18] = [0; std::mem::size_of::<BlockQ4_0>()];

#[repr(C)]
struct BlockQ4_1 {
    d: f16,
    m: f16,
    qs: [u8; QK4_1 / 2],
}
const _: [u8; 20] = [0; std::mem::size_of::<BlockQ4_1>()];

#[repr(C)]
struct BlockQ5_0 {
    d: f16,
    qh: [u8; 4],
    qs: [u8; QK5_0 / 2],
}
const _: [u8; 22] = [0; std::mem::size_of::<BlockQ5_0>()];

#[repr(C)]
struct BlockQ5_1 {
    d: f16,
    m: f16,
    qh: [u8; 4],
    qs: [u8; QK5_1 / 2],
}
const _: [u8; 24] = [0; std::mem::size_of::<BlockQ5_1>()];

#[repr(C)]
struct BlockQ8_0 {
    d: f16,
    qs: [u8; QK8_0],
}
const _: [u8; 34] = [0; std::mem::size_of::<BlockQ8_0>()];

#[repr(C)]
struct BlockQ8_1 {
    d: f16,
    s: f16,
    qs: [u8; QK8_1],
}
const _: [u8; 36] = [0; std::mem::size_of::<BlockQ8_1>()];

#[repr(C)]
struct BlockQ2K {
    scales: [u8; QK_K / 16],
    qs: [u8; QK_K / 4],
    d: f16,
    dmin: f16,
}
const _: [u8; QK_K / 16 + QK_K / 4 + 2 * 2] = [0; std::mem::size_of::<BlockQ2K>()];

#[repr(C)]
struct BlockQ3K {
    hmask: [u8; QK_K / 8],
    qs: [u8; QK_K / 4],
    scales: [u8; 12],
    d: f16,
}
const _: [u8; QK_K / 8 + QK_K / 4 + 12 + 2] = [0; std::mem::size_of::<BlockQ3K>()];

// https://github.com/ggerganov/llama.cpp/blob/468ea24fb4633a0d681f7ac84089566c1c6190cb/k_quants.h#L82
#[repr(C)]
struct BlockQ4K {
    d: f16,
    dmin: f16,
    scales: [u8; K_SCALE_SIZE],
    qs: [u8; QK_K / 2],
}
const _: [u8; QK_K / 2 + K_SCALE_SIZE + 2 * 2] = [0; std::mem::size_of::<BlockQ4K>()];

#[repr(C)]
struct BlockQ5K {
    d: f16,
    dmin: f16,
    scales: [u8; K_SCALE_SIZE],
    qh: [u8; QK_K / 8],
    qs: [u8; QK_K / 2],
}
const _: [u8; QK_K / 8 + QK_K / 2 + 2 * 2 + K_SCALE_SIZE] = [0; std::mem::size_of::<BlockQ5K>()];

#[repr(C)]
struct BlockQ6K {
    ql: [u8; QK_K / 2],
    qh: [u8; QK_K / 4],
    scales: [i8; QK_K / 16],
    d: f16,
}
const _: [u8; 3 * QK_K / 4 + QK_K / 16 + 2] = [0; std::mem::size_of::<BlockQ6K>()];

/*
            Self::Q2K => QK_K / 16 + QK_K / 4 + 2 * 2,
            Self::Q3K => QK_K / 8 + QK_K / 4 + 12 + 2,
            Self::Q4K => QK_K / 2 + K_SCALE_SIZE + 2 * 2,
            Self::Q5K => QK_K / 8 + QK_K / 2 + 2 * 2 + K_SCALE_SIZE,
            Self::Q6K => 3 * QK_K / 4 + QK_K / 16 + 2,
*/

// https://github.com/ggerganov/llama.cpp/blob/468ea24fb4633a0d681f7ac84089566c1c6190cb/llama.h#L37
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Magic {
    Ggjt,
    Ggla,
    Ggmf,
    Ggml,
    Ggsn,
}

impl TryFrom<u32> for Magic {
    type Error = crate::Error;
    fn try_from(value: u32) -> Result<Self> {
        let magic = match value {
            0x67676a74 => Self::Ggjt,
            0x67676c61 => Self::Ggla,
            0x67676d66 => Self::Ggmf,
            0x67676d6c => Self::Ggml,
            0x6767736e => Self::Ggsn,
            _ => crate::bail!("unknown magic {value:08x}"),
        };
        Ok(magic)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionedMagic {
    GgmlUnversioned,
    GgmfV1,
    GgjtV1,
    GgjtV2,
    GgjtV3,
}

impl VersionedMagic {
    fn read<R: std::io::Read>(reader: &mut R) -> Result<Self> {
        let magic = reader.read_u32::<LittleEndian>()?;
        let magic = Magic::try_from(magic)?;
        if magic == Magic::Ggml {
            return Ok(Self::GgmlUnversioned);
        }
        let version = reader.read_u32::<LittleEndian>()?;
        let versioned_magic = match (magic, version) {
            (Magic::Ggmf, 1) => Self::GgmfV1,
            (Magic::Ggjt, 1) => Self::GgjtV1,
            (Magic::Ggjt, 2) => Self::GgjtV2,
            (Magic::Ggjt, 3) => Self::GgjtV3,
            _ => crate::bail!("ggml: unsupported magic/version {magic:?}/{version}"),
        };
        Ok(versioned_magic)
    }

    fn align32(&self) -> bool {
        match self {
            Self::GgmlUnversioned | Self::GgmfV1 => false,
            Self::GgjtV1 | Self::GgjtV2 | Self::GgjtV3 => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HParams {
    pub n_vocab: u32,
    pub n_embd: u32,
    pub n_mult: u32,
    pub n_head: u32,
    pub n_layer: u32,
    pub n_rot: u32,
    pub ftype: u32,
}

impl HParams {
    fn read<R: std::io::Read>(reader: &mut R) -> Result<Self> {
        let n_vocab = reader.read_u32::<LittleEndian>()?;
        let n_embd = reader.read_u32::<LittleEndian>()?;
        let n_mult = reader.read_u32::<LittleEndian>()?;
        let n_head = reader.read_u32::<LittleEndian>()?;
        let n_layer = reader.read_u32::<LittleEndian>()?;
        let n_rot = reader.read_u32::<LittleEndian>()?;
        let ftype = reader.read_u32::<LittleEndian>()?;
        Ok(Self {
            n_vocab,
            n_embd,
            n_mult,
            n_head,
            n_layer,
            n_rot,
            ftype,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vocab {
    pub token_score_pairs: Vec<(Vec<u8>, f32)>,
}

impl Vocab {
    fn read<R: std::io::Read>(reader: &mut R, n_vocab: usize) -> Result<Self> {
        // https://github.com/ggerganov/llama.cpp/blob/468ea24fb4633a0d681f7ac84089566c1c6190cb/llama.cpp#L556
        let mut token_score_pairs = Vec::with_capacity(n_vocab);
        for _index in 0..n_vocab {
            let len = reader.read_u32::<LittleEndian>()? as usize;
            let mut word = vec![0u8; len];
            reader.read_exact(&mut word)?;
            let score = reader.read_f32::<LittleEndian>()?;
            token_score_pairs.push((word, score))
        }
        Ok(Self { token_score_pairs })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgmlDType {
    F32,
    F16,
    Q4_0,
    Q4_1,
    Q5_0,
    Q5_1,
    Q8_0,
    Q8_1,
    Q2K,
    Q3K,
    Q4K,
    Q5K,
    Q6K,
}

impl GgmlDType {
    fn from_u32(u: u32) -> Result<Self> {
        let dtype = match u {
            0 => Self::F32,
            1 => Self::F16,
            2 => Self::Q4_0,
            3 => Self::Q4_1,
            6 => Self::Q5_0,
            7 => Self::Q5_1,
            8 => Self::Q8_0,
            9 => Self::Q8_1,
            10 => Self::Q2K,
            11 => Self::Q3K,
            12 => Self::Q4K,
            13 => Self::Q5K,
            14 => Self::Q6K,
            _ => crate::bail!("unknown dtype for tensor {u}"),
        };
        Ok(dtype)
    }

    fn type_size(&self) -> usize {
        match self {
            Self::F32 => 4,
            Self::F16 => 2,
            Self::Q4_0 => std::mem::size_of::<BlockQ4_0>(),
            Self::Q4_1 => std::mem::size_of::<BlockQ4_1>(),
            Self::Q5_0 => std::mem::size_of::<BlockQ5_0>(),
            Self::Q5_1 => std::mem::size_of::<BlockQ5_1>(),
            // https://github.com/ggerganov/llama.cpp/blob/468ea24fb4633a0d681f7ac84089566c1c6190cb/ggml.c#L932
            Self::Q8_0 => std::mem::size_of::<BlockQ8_0>(),
            Self::Q8_1 => std::mem::size_of::<BlockQ8_1>(),
            Self::Q2K => std::mem::size_of::<BlockQ2K>(),
            Self::Q3K => std::mem::size_of::<BlockQ3K>(),
            Self::Q4K => std::mem::size_of::<BlockQ4K>(),
            Self::Q5K => std::mem::size_of::<BlockQ5K>(),
            Self::Q6K => std::mem::size_of::<BlockQ6K>(),
        }
    }

    fn blck_size(&self) -> usize {
        match self {
            Self::F32 => 1,
            Self::F16 => 1,
            Self::Q4_0 => QK4_0,
            Self::Q4_1 => QK4_1,
            Self::Q5_0 => QK5_0,
            Self::Q5_1 => QK5_1,
            Self::Q8_0 => QK8_0,
            Self::Q8_1 => QK8_1,
            Self::Q2K | Self::Q3K | Self::Q4K | Self::Q5K | Self::Q6K => QK_K,
        }
    }
}

#[derive(Debug)]
pub struct Content {
    pub magic: VersionedMagic,
    pub hparams: HParams,
    pub vocab: Vocab,
}

impl Content {
    pub fn read<R: std::io::Seek + std::io::Read>(reader: &mut R) -> Result<Content> {
        // https://github.com/ggerganov/llama.cpp/blob/468ea24fb4633a0d681f7ac84089566c1c6190cb/llama.cpp#L505
        let last_position = reader.seek(std::io::SeekFrom::End(0))?;
        reader.seek(std::io::SeekFrom::Start(0))?;
        let magic = VersionedMagic::read(reader)?;
        let hparams = HParams::read(reader)?;
        let vocab = Vocab::read(reader, hparams.n_vocab as usize)?;

        while reader.stream_position()? != last_position {
            let n_dims = reader.read_u32::<LittleEndian>()?;
            let name_len = reader.read_u32::<LittleEndian>()?;
            let dtype = reader.read_u32::<LittleEndian>()?;
            let dtype = GgmlDType::from_u32(dtype)?;
            let mut dims = vec![0u32; n_dims as usize];
            reader.read_u32_into::<LittleEndian>(&mut dims)?;
            let mut name = vec![0u8; name_len as usize];
            reader.read_exact(&mut name)?;
            let name = String::from_utf8_lossy(&name).into_owned();

            if magic.align32() {
                let pos = reader.stream_position()?;
                reader.seek(std::io::SeekFrom::Current(((32 - pos % 32) % 32) as i64))?;
            }
            let tensor_elems = dims.iter().map(|&u| u as usize).product::<usize>();
            let tensor_size = tensor_elems * dtype.type_size() / dtype.blck_size();
            println!("{name} {dtype:?} {dims:?}");
            reader.seek(std::io::SeekFrom::Current(tensor_size as i64))?;
        }
        Ok(Self {
            magic,
            hparams,
            vocab,
        })
    }
}