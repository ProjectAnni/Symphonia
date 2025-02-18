// Symphonia
// Copyright (c) 2019-2023 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt;

use symphonia_core::audio::Channels;
use symphonia_core::codecs::{
    CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S32BE, CODEC_TYPE_PCM_S8,
};
use symphonia_core::errors::{decode_error, unsupported_error, Result};
use symphonia_core::io::ReadBytes;

use crate::{ChunkParser, FormatData, FormatPcm, PacketInfo, ParseChunk, ParseChunkTag};

use extended::Extended;

/// `CommonChunk` is a required AIFF chunk, containing metadata.
pub struct CommonChunk {
    /// The number of channels.
    pub n_channels: i16,
    /// The number of audio frames.
    pub n_sample_frames: u32,
    /// The sample size in bits.
    pub sample_size: i16,
    /// The sample rate in Hz.
    pub sample_rate: u32,
    /// Extra data associated with the format block conditional upon the format tag.
    pub format_data: FormatData,
}

impl CommonChunk {
    fn read_pcm_fmt(bits_per_sample: u16, n_channels: u16) -> Result<FormatData> {
        // Bits per sample for PCM is both the encoded sample width, and the actual sample width.
        // Strictly, this must either be 8 or 16 bits, but there is no reason why 24 and 32 bits
        // can't be supported. Since these files do exist, allow for 8/16/24/32-bit samples, but
        // error if not a multiple of 8 or greater than 32-bits.
        //
        // It is possible though for AIFF to have a sample size not divisible by 8.
        // Data is left justified, with the remaining bits zeroed. Currently not supported.
        //
        // Select the appropriate codec using bits per sample. Samples are always interleaved and
        // little-endian encoded for the PCM format.
        let codec = match bits_per_sample {
            8 => CODEC_TYPE_PCM_S8,
            16 => CODEC_TYPE_PCM_S16BE,
            24 => CODEC_TYPE_PCM_S24BE,
            32 => CODEC_TYPE_PCM_S32BE,
            _ => return decode_error("aiff: bits per sample for pcm must be 8, 16, 24 or 32 bits"),
        };

        // The PCM format only supports 1 or 2 channels, for mono and stereo channel layouts,
        // respectively.
        let channels = match n_channels {
            1 => Channels::FRONT_LEFT,
            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            _ => return decode_error("aiff: channel layout is not stereo or mono for fmt_pcm"),
        };

        Ok(FormatData::Pcm(FormatPcm { bits_per_sample, channels, codec }))
    }

    pub fn packet_info(&self) -> Result<PacketInfo> {
        match &self.format_data {
            FormatData::Pcm(_) => {
                let block_align = self.n_channels * self.sample_size / 8;
                Ok(PacketInfo::without_blocks(block_align as u16))
            }
            _ => return unsupported_error("aiff: packet info not implemented for format"),
        }
    }
}

impl ParseChunk for CommonChunk {
    fn parse<B: ReadBytes>(reader: &mut B, _tag: [u8; 4], _: u32) -> Result<CommonChunk> {
        let n_channels = reader.read_be_i16()?;
        let n_sample_frames = reader.read_be_u32()?;
        let sample_size = reader.read_be_i16()?;

        let mut sample_rate: [u8; 10] = [0; 10];
        let _res = reader.read_buf_exact(sample_rate.as_mut())?;

        let sample_rate = Extended::from_be_bytes(sample_rate);
        let sample_rate = sample_rate.to_f64() as u32;

        let format_data = Self::read_pcm_fmt(sample_size as u16, n_channels as u16);

        let format_data = match format_data {
            Ok(data) => data,
            Err(e) => return Err(e),
        };
        Ok(CommonChunk { n_channels, n_sample_frames, sample_size, sample_rate, format_data })
    }
}

impl fmt::Display for CommonChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "CommonChunk {{")?;
        writeln!(f, "\tn_channels: {},", self.n_channels)?;
        writeln!(f, "\tsample_rate: {} Hz,", self.sample_rate)?;

        match self.format_data {
            FormatData::Pcm(ref pcm) => {
                writeln!(f, "\tformat_data: Pcm {{")?;
                writeln!(f, "\t\tbits_per_sample: {},", pcm.bits_per_sample)?;
                writeln!(f, "\t\tchannels: {},", pcm.channels)?;
                writeln!(f, "\t\tcodec: {},", pcm.codec)?;
            }
            _ => {
                //TODO: this is not optimal..
                writeln!(f, "\tdisplay not implemented for format")?;
            }
        };

        writeln!(f, "\t}}")?;
        writeln!(f, "}}")
    }
}

/// `SoundChunk` is a required AIFF chunk, containing the audio data.
pub struct SoundChunk {
    pub len: u32,
    pub offset: u32,
    pub block_size: u32,
}

impl ParseChunk for SoundChunk {
    fn parse<B: ReadBytes>(reader: &mut B, _: [u8; 4], len: u32) -> Result<SoundChunk> {
        let offset = reader.read_be_u32()?;
        let block_size = reader.read_be_u32()?;

        if offset != 0 || block_size != 0 {
            return unsupported_error("riff: No support for AIFF block-aligned data");
        }

        Ok(SoundChunk { len, offset, block_size })
    }
}

pub enum RiffAiffChunks {
    Common(ChunkParser<CommonChunk>),
    Sound(ChunkParser<SoundChunk>),
}

macro_rules! parser {
    ($class:expr, $result:ty, $tag:expr, $len:expr) => {
        Some($class(ChunkParser::<$result>::new($tag, $len)))
    };
}

impl ParseChunkTag for RiffAiffChunks {
    fn parse_tag(tag: [u8; 4], len: u32) -> Option<Self> {
        match &tag {
            b"COMM" => parser!(RiffAiffChunks::Common, CommonChunk, tag, len),
            b"SSND" => parser!(RiffAiffChunks::Sound, SoundChunk, tag, len),
            _ => None,
        }
    }
}
