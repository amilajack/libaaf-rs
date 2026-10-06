//! Media resolution and buffered extraction.
use crate::{
    Document, Error, Result,
    bytes::*,
    timeline::{Composition, EssenceFormat, Rational},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_size: u16,
    pub frames: u64,
    pub data_offset: u64,
    pub data_size: u64,
    pub big_endian: bool,
}
pub fn audio_info(bytes: &[u8]) -> Result<AudioInfo> {
    parse_info(|at, len| Ok(slice(bytes, at as usize, len)?.to_vec()))
}
pub fn audio_info_file(path: &Path) -> Result<AudioInfo> {
    let mut file = File::open(path)?;
    parse_info(|at, len| {
        file.seek(SeekFrom::Start(at))?;
        let mut b = vec![0; len];
        file.read_exact(&mut b)?;
        Ok(b)
    })
}
fn parse_info(mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>) -> Result<AudioInfo> {
    let hdr = read(0, 12)?;
    let be = matches!(&hdr[8..12], b"AIFF" | b"AIFC");
    if !be && &hdr[8..12] != b"WAVE" {
        return Err(Error::Unsupported("media container".into()));
    }
    let total = if be {
        u32::from_be_bytes(hdr[4..8].try_into().unwrap())
    } else {
        u32_at(&hdr, 4)?
    } as u64
        + 8;
    let mut info = AudioInfo {
        channels: 0,
        sample_rate: 0,
        sample_size: 0,
        frames: 0,
        data_offset: 0,
        data_size: 0,
        big_endian: be,
    };
    let mut at = 12u64;
    while at < total {
        let Ok(h) = read(at, 8) else {
            break;
        };
        let size = if be {
            u32::from_be_bytes(h[4..8].try_into().unwrap())
        } else {
            u32_at(&h, 4)?
        } as u64;
        match &h[..4] {
            b"fmt " if !be => {
                let b = read(at + 8, 16)?;
                info.channels = u16_at(&b, 2)?;
                info.sample_rate = u32_at(&b, 4)?;
                info.sample_size = u16_at(&b, 14)?;
            }
            b"COMM" if be => {
                let b = read(at + 8, 18)?;
                info.channels = u16::from_be_bytes([b[0], b[1]]);
                info.sample_size = u16::from_be_bytes([b[6], b[7]]);
                let exponent = (u16::from_be_bytes([b[8], b[9]]) & 0x7fff) as i32 - 16383;
                let mantissa = u64::from_be_bytes(b[10..18].try_into().unwrap());
                info.sample_rate = ((mantissa as f64) * 2.0f64.powi(exponent - 63)) as u32;
                info.frames = u32::from_be_bytes(b[2..6].try_into().unwrap()) as u64;
            }
            b"data" if !be => {
                info.data_offset = at + 8;
                info.data_size = size;
                break;
            }
            b"SSND" if be => {
                info.data_offset = at + 16;
                info.data_size = size.saturating_sub(8);
                break;
            }
            _ => {}
        }
        at = at
            .checked_add(8)
            .and_then(|v| v.checked_add(size))
            .and_then(|v| v.checked_add(size % 2))
            .ok_or_else(|| Error::invalid("RIFF chunk size", at as usize))?;
    }
    if info.channels == 0
        || info.sample_size == 0
        || info.sample_rate == 0
        || !info.sample_size.is_multiple_of(8)
    {
        return Err(Error::invalid("audio format", 0));
    }
    // LibAAF counts SSND's eight-byte offset/block header as samples in summaries.
    // Retain that interpretation for display; extraction uses the actual data bounds.
    if info.data_offset > 0 {
        info.frames = (info.data_size + if be { 8 } else { 0 })
            / (info.channels as u64 * (info.sample_size as u64 / 8));
    }
    Ok(info)
}
pub fn decode_uri(uri: &str) -> Result<String> {
    let path = if let Some(rest) = uri.strip_prefix("file://") {
        if rest.starts_with('/') {
            rest
        } else {
            rest.find('/').map(|i| &rest[i..]).unwrap_or("")
        }
    } else {
        uri
    };
    let b = path.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let s = path
                .get(i + 1..i + 3)
                .ok_or_else(|| Error::invalid("URI percent escape", i))?;
            out.push(
                u8::from_str_radix(s, 16).map_err(|_| Error::invalid("URI percent escape", i))?,
            );
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .map(|s| s.replace('\\', "/"))
        .map_err(|_| Error::invalid("URI UTF-8", 0))
}
pub fn resolve_media(original: &str, base: &Path, common: Option<&str>) -> Result<Option<PathBuf>> {
    let decoded = decode_uri(original)?;
    let mut candidates = Vec::new();
    if let Some(common) = common {
        let common = decode_uri(common)?;
        if let Some(parent) = common.trim_end_matches('/').rfind('/')
            && let Some(rest) = decoded.get(parent..)
        {
            candidates.push(base.join(rest.trim_start_matches('/')));
        }
        if let Some(rest) = decoded.strip_prefix(&common) {
            candidates.push(base.join(rest.trim_start_matches('/')));
        }
    } else {
        for (i, c) in decoded.char_indices().rev() {
            if c == '/' {
                candidates.push(base.join(&decoded[i + 1..]));
            }
        }
        candidates.push(base.join(decoded.trim_start_matches('/')));
    }
    candidates.push(PathBuf::from(&decoded));
    for path in candidates {
        if path.is_file() {
            return Ok(Some(path.canonicalize()?));
        }
    }
    Ok(None)
}
pub(crate) fn resolve_all(doc: &Document, c: &mut Composition) -> Result<()> {
    let paths: Vec<_> = c
        .essences
        .iter()
        .filter(|e| e.embedded_node.is_none() && e.format != EssenceFormat::Video)
        .filter_map(|e| e.original_path.as_deref())
        .collect();
    let common = if paths.len() > 1 {
        let mut common = paths[0];
        for p in &paths[1..] {
            let bytes = common
                .as_bytes()
                .iter()
                .zip(p.as_bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let mut end = bytes;
            while !common.is_char_boundary(end) {
                end -= 1;
            }
            common = &common[..end];
        }
        common.rfind('/').map(|i| common[..i + 1].to_owned())
    } else {
        None
    };
    let base = doc
        .options
        .media_location
        .as_deref()
        .or_else(|| doc.path.as_deref().and_then(Path::parent))
        .unwrap_or_else(|| Path::new("."));
    for e in &mut c.essences {
        if e.embedded_node.is_none()
            && let Some(path) = &e.original_path
        {
            e.usable_path = resolve_media(path, base, common.as_deref())?;
        }
        if let Some(path) = &e.usable_path
            && let Ok(info) = audio_info_file(path)
        {
            e.channels = info.channels;
            e.sample_rate = info.sample_rate;
            e.sample_size = info.sample_size;
            e.length = info.frames as i64;
            e.edit_rate = Rational::new(info.sample_rate as i32, 1);
            e.data_offset = info.data_offset;
        }
    }
    Ok(())
}

use crate::{
    defs::*,
    timeline::{Clip, Essence, Track},
};
use std::io::{self, BufWriter, Write};
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum ExtractFormat {
    #[default]
    Original,
    Wave,
    BroadcastWave,
}
/// Frame range in the media's sample rate. None preserves the whole essence.
#[derive(Debug, Clone, Copy)]
pub struct FrameRange {
    pub offset: u64,
    pub length: u64,
}
pub fn extraction_filename(e: &Essence, format: ExtractFormat, clip_name: Option<&str>) -> String {
    let name = clip_name.unwrap_or(&e.unique_name);
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let wav =
        format != ExtractFormat::Original || e.format != EssenceFormat::Aiff || clip_name.is_some();
    let mut name = if wav {
        if ["wav", "wave"].contains(&ext.as_str()) {
            name.into()
        } else {
            format!("{name}.wav")
        }
    } else if ["aif", "aiff", "aifc"].contains(&ext.as_str()) {
        name.into()
    } else {
        format!("{name}.aif")
    };
    name = name
        .chars()
        .map(|c| {
            if c < ' ' || "/<>:\"|?*\\".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    name.trim_end_matches([' ', '.']).to_owned()
}
struct Extraction<'a> {
    stream: crate::cfb::Stream<'a>,
    header: Vec<u8>,
    start: usize,
    length: usize,
    swap_sample: usize,
}
impl Extraction<'_> {
    fn write(self, mut out: impl Write) -> Result<u64> {
        out.write_all(&self.header)?;
        if self.swap_sample == 0 {
            self.stream.copy_range(self.start, self.length, &mut out)?;
        } else {
            let mut reader = self.stream.reader();
            io::copy(
                &mut reader.by_ref().take(self.start as u64),
                &mut io::sink(),
            )?;
            let mut left = self.length;
            let mut buffer = vec![0; 65536 - (65536 % self.swap_sample)];
            while left > 0 {
                let n = left.min(buffer.len());
                reader.read_exact(&mut buffer[..n])?;
                for b in buffer[..n].chunks_exact_mut(self.swap_sample) {
                    b.reverse();
                }
                out.write_all(&buffer[..n])?;
                left -= n;
            }
        }
        Ok(self.header.len() as u64 + self.length as u64)
    }
}
fn prepare_extraction<'a>(
    doc: &'a Document,
    c: &Composition,
    index: usize,
    format: ExtractFormat,
    range: Option<FrameRange>,
) -> Result<Extraction<'a>> {
    let e = c
        .essences
        .get(index)
        .ok_or_else(|| Error::invalid("essence index", index))?;
    let node = e.embedded_node.ok_or_else(|| Error::Missing {
        context: "embedded media",
        name: e.name.clone(),
    })?;
    let stream = doc.cfb.stream(node)?;
    let rewrite =
        format != ExtractFormat::Original || range.is_some() || e.format == EssenceFormat::Pcm;
    if !rewrite {
        let length = stream.size;
        return Ok(Extraction {
            stream,
            header: Vec::new(),
            start: 0,
            length,
            swap_sample: 0,
        });
    }
    if e.channels == 0
        || e.sample_rate == 0
        || e.sample_size == 0
        || !e.sample_size.is_multiple_of(8)
    {
        return Err(Error::invalid("sample frame size", 0));
    }
    let block = (e.channels as u64)
        .checked_mul(e.sample_size as u64 / 8)
        .ok_or_else(|| Error::invalid("sample frame size", 0))?;
    let (offset, length) = if let Some(r) = range {
        (
            r.offset
                .checked_mul(block)
                .ok_or_else(|| Error::invalid("sample offset", 0))?,
            Some(
                r.length
                    .checked_mul(block)
                    .ok_or_else(|| Error::invalid("sample length", 0))?,
            ),
        )
    } else {
        (0, None)
    };
    let start = e
        .data_offset
        .checked_add(offset)
        .ok_or_else(|| Error::invalid("media offset", 0))?;
    if start > stream.size as u64 {
        return Err(Error::invalid("media offset", start as usize));
    }
    let len = length.unwrap_or(stream.size as u64 - start);
    if start
        .checked_add(len)
        .is_none_or(|end| end > stream.size as u64)
    {
        return Err(Error::invalid("clip past media end", start as usize));
    }
    let size =
        u32::try_from(len).map_err(|_| Error::Unsupported("WAV larger than 4 GiB".into()))?;
    let bext = if format != ExtractFormat::Wave {
        Some(broadcast_metadata(doc, c, e))
    } else {
        None
    };
    let header = wave_header(e, size, bext.as_deref())?;
    let swap_sample = if e.format == EssenceFormat::Aiff && e.sample_size > 8 {
        let sample = e.sample_size as usize / 8;
        if !(2..=4).contains(&sample) || !(len as usize).is_multiple_of(sample) {
            return Err(Error::Unsupported("AIFF sample size".into()));
        }
        sample
    } else {
        0
    };
    // The pinned C converter selects pcmByteOffset rather than sourceFileOffset
    // for AIFF. Preserve that documented quirk, covered by extraction hashes.
    let selected = if swap_sample > 0 { offset } else { start };
    Ok(Extraction {
        stream,
        header,
        start: selected as usize,
        length: len as usize,
        swap_sample,
    })
}
/// Extract to a caller-provided buffered writer. All bounds are validated first.
/// AIFF conversion follows the pinned reference's byte-selection behavior.
pub fn extract_essence(
    doc: &Document,
    c: &Composition,
    index: usize,
    format: ExtractFormat,
    range: Option<FrameRange>,
    out: impl Write,
) -> Result<u64> {
    prepare_extraction(doc, c, index, format, range)?.write(out)
}
fn wave_header(e: &Essence, size: u32, bext: Option<&[u8]>) -> Result<Vec<u8>> {
    let extra = bext.map(|b| b.len() as u32 + 8).unwrap_or(0);
    let total = size
        .checked_add(36)
        .and_then(|v| v.checked_add(extra))
        .ok_or_else(|| Error::invalid("RIFF size", 0))?;
    let block = e
        .channels
        .checked_mul(e.sample_size / 8)
        .ok_or_else(|| Error::invalid("WAV block size", 0))?;
    let byte_rate = e
        .sample_rate
        .checked_mul(block as u32)
        .ok_or_else(|| Error::invalid("WAV byte rate", 0))?;
    let mut b = Vec::with_capacity(44 + extra as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&total.to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&e.channels.to_le_bytes());
    b.extend_from_slice(&e.sample_rate.to_le_bytes());
    b.extend_from_slice(&byte_rate.to_le_bytes());
    b.extend_from_slice(&block.to_le_bytes());
    b.extend_from_slice(&e.sample_size.to_le_bytes());
    if let Some(bext) = bext {
        b.extend_from_slice(b"bext");
        b.extend_from_slice(&(bext.len() as u32).to_le_bytes());
        b.extend_from_slice(bext);
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&size.to_le_bytes());
    Ok(b)
}
fn broadcast_metadata(doc: &Document, c: &Composition, e: &Essence) -> Vec<u8> {
    let mut b = vec![0; 602];
    fn field(b: &mut [u8], s: &str) {
        let n = s.len().min(b.len().saturating_sub(1));
        b[..n].copy_from_slice(&s.as_bytes()[..n]);
    }
    field(
        &mut b[..256],
        &format!("{}\n{}.aaf", e.name, c.name.as_deref().unwrap_or("(null)")),
    );
    if let Some(id) = doc.identification {
        let o = doc.object(id);
        let product = o.string(PID_Identification_ProductName).unwrap_or_default();
        let version = if product.starts_with("Avid Media Composer") {
            String::new()
        } else {
            o.string(PID_Identification_ProductVersionString)
                .unwrap_or_default()
        };
        field(&mut b[256..288], &format!("{product} {version}"));
    }
    field(&mut b[288..320], "libAAF v1.0-30-g7925dff");
    if let Some(t) = doc
        .object(e.source_mob)
        .raw(PID_Mob_CreationTime)
        .filter(|b| b.len() >= 8)
    {
        let date = format!("{:04}:{:02}:{:02}", u16_at(t, 0).unwrap(), t[2], t[3]);
        b[320..330].copy_from_slice(&date.as_bytes()[..10]);
        let time = format!("{:02}:{:02}:{:02}", t[4], t[5], t[6]);
        b[330..338].copy_from_slice(&time.as_bytes()[..8]);
    }
    let source_rate = doc
        .object(e.source_mob)
        .references(PID_Mob_Slots)
        .iter()
        .find_map(|&id| {
            let o = doc.object(id);
            (o.u32(PID_MobSlot_SlotID) == Some(e.source_slot))
                .then(|| o.rational(PID_TimelineMobSlot_EditRate))
                .flatten()
        })
        .unwrap_or(e.edit_rate);
    b[338..346].copy_from_slice(&(source_rate.convert(e.origin, e.edit_rate) as u64).to_le_bytes());
    b[346..348].copy_from_slice(&1u16.to_le_bytes());
    b[348..380].copy_from_slice(&e.mob_id);
    b
}
pub fn clip_range(clip: &Clip, track: &Track, essence: &Essence) -> Result<FrameRange> {
    let offset = track
        .edit_rate
        .convert(clip.source_offset, essence.edit_rate);
    let length = track.edit_rate.convert(clip.length, essence.edit_rate);
    if offset < 0 || length < 0 {
        return Err(Error::invalid("negative extraction range", 0));
    }
    Ok(FrameRange {
        offset: offset as u64,
        length: length as u64,
    })
}
pub fn extract_to_file(
    doc: &Document,
    c: &Composition,
    index: usize,
    format: ExtractFormat,
    range: Option<FrameRange>,
    path: &Path,
) -> Result<u64> {
    let extraction = prepare_extraction(doc, c, index, format, range)?;
    let mut out = BufWriter::new(File::create(path)?);
    let n = extraction.write(&mut out)?;
    out.flush()?;
    Ok(n)
}
