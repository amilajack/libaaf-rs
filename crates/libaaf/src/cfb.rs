//! Checked Compound File Binary storage with borrowed, fragmented streams.
use crate::{Error, Result, Uid, bytes::*};
use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    io::{self, Read, Write},
    ops::Range,
    path::Path,
};
#[derive(Clone, Copy)]
struct ChildIndex {
    parent: u32,
    node: u32,
    name_hash: u64,
}
const FREE: u32 = 0xffff_ffff;
const END: u32 = 0xffff_fffe;
const FAT: u32 = 0xffff_fffd;
const DIF: u32 = 0xffff_fffc;

fn folded_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(b.bytes().map(|b| b.to_ascii_lowercase()));
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

#[derive(Clone, Debug)]
pub struct Header {
    pub class: Uid,
    pub minor: u16,
    pub major: u16,
    pub sector_size: usize,
    pub mini_sector_size: usize,
    pub directory_sectors: u32,
    pub fat_sectors: u32,
    pub first_directory: u32,
    pub transaction: u32,
    pub mini_cutoff: u32,
    pub first_minifat: u32,
    pub minifat_sectors: u32,
    pub first_difat: u32,
    pub difat_sectors: u32,
}
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub kind: u8,
    pub color: u8,
    pub left: u32,
    pub right: u32,
    pub child: u32,
    pub class: Uid,
    pub state: u32,
    pub created: u64,
    pub modified: u64,
    pub start: u32,
    pub size: u64,
    pub parent: Option<u32>,
}
/// Owns the file bytes; stream views refer to ranges within this allocation.
pub struct CompoundFile {
    data: Vec<u8>,
    pub header: Header,
    pub nodes: Vec<Node>,
    pub fat: Vec<u32>,
    pub minifat: Vec<u32>,
    pub difat: Vec<u32>,
    mini_chain: Vec<u32>,
    // Hashes index the one owned name; collisions use full folded-name comparison.
    children: Vec<ChildIndex>,
    name_hasher: RandomState,
}
impl CompoundFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        slice(&data, 0, 512)?;
        if slice(&data, 0, 8)? != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
            return Err(Error::invalid("CFB signature", 0));
        }
        if u16_at(&data, 28)? != 0xfffe {
            return Err(Error::invalid("CFB byte order", 28));
        }
        let major = u16_at(&data, 26)?;
        let shift = u16_at(&data, 30)?;
        if !matches!((major, shift), (3, 9) | (4, 12)) || u16_at(&data, 32)? != 6 {
            return Err(Error::invalid("CFB sector size", 30));
        }
        let header = Header {
            class: Uid::from_bytes(&data[8..24]).unwrap(),
            minor: u16_at(&data, 24)?,
            major,
            sector_size: 1usize << shift,
            mini_sector_size: 64,
            directory_sectors: u32_at(&data, 40)?,
            fat_sectors: u32_at(&data, 44)?,
            first_directory: u32_at(&data, 48)?,
            transaction: u32_at(&data, 52)?,
            mini_cutoff: u32_at(&data, 56)?,
            first_minifat: u32_at(&data, 60)?,
            minifat_sectors: u32_at(&data, 64)?,
            first_difat: u32_at(&data, 68)?,
            difat_sectors: u32_at(&data, 72)?,
        };
        if header.mini_cutoff != 4096 {
            return Err(Error::invalid("CFB mini cutoff", 56));
        }
        let sectors = data.len() / header.sector_size;
        if sectors == 0 || !data.len().is_multiple_of(header.sector_size) {
            return Err(Error::invalid("CFB file size", data.len()));
        }
        let max = sectors - 1;
        if header.fat_sectors as usize > max
            || header.difat_sectors as usize > max
            || header.minifat_sectors as usize > max
        {
            return Err(Error::invalid("CFB sector count", 44));
        }
        let mut file = Self {
            data,
            header,
            nodes: Vec::new(),
            fat: Vec::new(),
            minifat: Vec::new(),
            difat: Vec::new(),
            mini_chain: Vec::new(),
            children: Vec::new(),
            name_hasher: RandomState::new(),
        };
        for i in 0..109 {
            let id = u32_at(&file.data, 76 + i * 4)?;
            if id != FREE {
                file.difat.push(id);
            }
        }
        let mut current = file.header.first_difat;
        let mut visited = vec![false; max];
        for _ in 0..file.header.difat_sectors {
            if current as usize >= max {
                return Err(Error::invalid("DIFAT sector", current as usize));
            }
            if std::mem::replace(&mut visited[current as usize], true) {
                return Err(Error::Cycle {
                    context: "DIFAT",
                    id: current,
                });
            }
            let sector = file.sector(current)?;
            let ids: Vec<u32> = sector
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| u32::from_le_bytes(*v))
                .collect();
            current = *ids.last().unwrap();
            file.difat.extend(
                ids[..ids.len() - 1]
                    .iter()
                    .copied()
                    .filter(|id| *id != FREE),
            );
        }
        file.difat.truncate(file.header.fat_sectors as usize);
        if file.difat.len() != file.header.fat_sectors as usize {
            return Err(Error::invalid("DIFAT length", 44));
        }
        let mut used = vec![false; max];
        for &id in &file.difat {
            if id as usize >= max {
                return Err(Error::invalid("FAT sector", id as usize));
            }
            if std::mem::replace(&mut used[id as usize], true) {
                return Err(Error::Cycle {
                    context: "FAT sector list",
                    id,
                });
            }
            let values: Vec<u32> = file
                .sector(id)?
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| u32::from_le_bytes(*v))
                .collect();
            file.fat.extend(values);
        }
        if file.fat.len() < max {
            return Err(Error::invalid("FAT length", 0));
        }
        for &id in &file.difat {
            if file.fat[id as usize] != FAT {
                return Err(Error::invalid("FAT marker", id as usize));
            }
        }
        let dir = file.chain(file.header.first_directory, &file.fat, max)?;
        let mut directory = Vec::with_capacity(dir.len() * file.header.sector_size);
        for id in dir {
            directory.extend_from_slice(file.sector(id)?);
        }
        file.nodes.reserve_exact(directory.len() / 128);
        for (i, b) in directory.as_chunks::<128>().0.iter().enumerate() {
            let kind = b[66];
            let nlen = u16_at(b, 64)? as usize;
            let name = if kind == 0 {
                String::new()
            } else {
                if !(2..=64).contains(&nlen) || !nlen.is_multiple_of(2) || u16_at(b, nlen - 2)? != 0
                {
                    return Err(Error::invalid("directory name", i * 128));
                }
                utf16(&b[..nlen])?
            };
            if !matches!(kind, 0 | 1 | 2 | 5) {
                return Err(Error::invalid("directory type", i * 128 + 66));
            }
            let size = if major == 3 {
                u32_at(b, 120)? as u64
            } else {
                u64_at(b, 120)?
            };
            file.nodes.push(Node {
                id: i as u32,
                name,
                kind,
                color: b[67],
                left: u32_at(b, 68)?,
                right: u32_at(b, 72)?,
                child: u32_at(b, 76)?,
                class: Uid::from_bytes(&b[80..96]).unwrap(),
                state: u32_at(b, 96)?,
                created: u64_at(b, 100)?,
                modified: u64_at(b, 108)?,
                start: u32_at(b, 116)?,
                size,
                parent: None,
            });
        }
        if file.nodes.first().is_none_or(|n| n.kind != 5) {
            return Err(Error::invalid("root directory", 0));
        }
        drop(directory);
        file.children
            .reserve_exact(file.nodes.len().saturating_sub(1));
        let mut seen = vec![false; file.nodes.len()];
        seen[0] = true;
        let mut stack = vec![(file.nodes[0].child, 0)];
        while let Some((id, parent)) = stack.pop() {
            if id == FREE {
                continue;
            }
            let n = file
                .nodes
                .get_mut(id as usize)
                .ok_or_else(|| Error::invalid("directory ID", id as usize))?;
            if std::mem::replace(&mut seen[id as usize], true) {
                return Err(Error::Cycle {
                    context: "directory",
                    id,
                });
            }
            if n.kind == 0 || n.kind == 5 {
                return Err(Error::invalid("directory child type", id as usize));
            }
            n.parent = Some(parent);
            file.children.push(ChildIndex {
                parent,
                node: id,
                name_hash: file.name_hasher.hash_one(n.name.to_lowercase()),
            });
            stack.push((n.right, parent));
            stack.push((n.left, parent));
            if n.kind == 1 {
                stack.push((n.child, id));
            }
        }
        let key_cmp = |a: ChildIndex, b: ChildIndex| {
            (a.parent, a.name_hash)
                .cmp(&(b.parent, b.name_hash))
                .then_with(|| {
                    folded_cmp(
                        &file.nodes[a.node as usize].name,
                        &file.nodes[b.node as usize].name,
                    )
                })
        };
        file.children.sort_unstable_by(|&a, &b| key_cmp(a, b));
        for pair in file.children.windows(2) {
            if key_cmp(pair[0], pair[1]).is_eq() {
                return Err(Error::invalid(
                    "duplicate directory name",
                    pair[1].node as usize,
                ));
            }
        }
        let mini_fat = file.chain(file.header.first_minifat, &file.fat, max)?;
        if mini_fat.len() != file.header.minifat_sectors as usize {
            return Err(Error::invalid("MiniFAT length", 64));
        }
        for id in mini_fat {
            let values: Vec<u32> = file
                .sector(id)?
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| u32::from_le_bytes(*v))
                .collect();
            file.minifat.extend(values);
        }
        file.mini_chain = file.chain(file.nodes[0].start, &file.fat, max)?;
        let root_size = usize::try_from(file.nodes[0].size)
            .map_err(|_| Error::invalid("root stream size", 0))?;
        if root_size > file.mini_chain.len() * file.header.sector_size {
            return Err(Error::invalid("root stream length", 0));
        }
        Ok(file)
    }
    fn sector_range(&self, id: u32) -> Result<Range<usize>> {
        let start = (id as usize)
            .checked_add(1)
            .and_then(|v| v.checked_mul(self.header.sector_size))
            .ok_or_else(|| Error::invalid("sector overflow", id as usize))?;
        slice(&self.data, start, self.header.sector_size)?;
        Ok(start..start + self.header.sector_size)
    }
    pub fn sector(&self, id: u32) -> Result<&[u8]> {
        Ok(&self.data[self.sector_range(id)?])
    }
    fn chain(&self, start: u32, table: &[u32], max: usize) -> Result<Vec<u32>> {
        let mut ids = Vec::new();
        let mut id = start;
        // A chain may visit at most the number of addressable sectors. A repeated
        // sector therefore fails even when no scratch visited bitmap is allocated.
        while id != END {
            if id as usize >= max || matches!(id, FREE | FAT | DIF) {
                return Err(Error::invalid("sector chain", id as usize));
            }
            if ids.len() >= max {
                return Err(Error::Cycle {
                    context: "sector chain",
                    id,
                });
            }
            ids.push(id);
            id = *table
                .get(id as usize)
                .ok_or_else(|| Error::invalid("allocation table", id as usize))?;
        }
        Ok(ids)
    }
    pub fn child(&self, parent: u32, name: &str) -> Option<u32> {
        let key = (parent, self.name_hasher.hash_one(name.to_lowercase()));
        let mut index = self
            .children
            .binary_search_by_key(&key, |entry| (entry.parent, entry.name_hash))
            .ok()?;
        while index > 0
            && (
                self.children[index - 1].parent,
                self.children[index - 1].name_hash,
            ) == key
        {
            index -= 1;
        }
        self.children[index..]
            .iter()
            .take_while(|entry| (entry.parent, entry.name_hash) == key)
            .find(|entry| folded_cmp(&self.nodes[entry.node as usize].name, name).is_eq())
            .map(|entry| entry.node)
    }
    pub fn node_by_path(&self, path: &str) -> Option<u32> {
        let mut id = 0;
        let mut parts = path.split(['/', '\\']).filter(|s| !s.is_empty()).peekable();
        if parts.peek().is_some_and(|v| {
            v.eq_ignore_ascii_case(&self.nodes[0].name) || v.eq_ignore_ascii_case("Root")
        }) {
            parts.next();
        }
        for p in parts {
            id = self.child(id, p)?;
        }
        Some(id)
    }
    pub fn path(&self, id: u32) -> String {
        let mut names = Vec::new();
        let mut id = Some(id);
        while let Some(i) = id {
            if let Some(n) = self.nodes.get(i as usize) {
                names.push(n.name.as_str());
                id = n.parent;
            } else {
                break;
            }
        }
        names.reverse();
        format!("/{}", names.join("/"))
    }
    pub fn stream(&self, id: u32) -> Result<Stream<'_>> {
        let n = self
            .nodes
            .get(id as usize)
            .ok_or_else(|| Error::invalid("stream ID", id as usize))?;
        if n.kind != 2 && n.kind != 5 {
            return Err(Error::invalid("non-stream node", id as usize));
        }
        if n.size == 0 {
            return Ok(Stream {
                file: self,
                ranges: Vec::new(),
                size: 0,
            });
        }
        let size =
            usize::try_from(n.size).map_err(|_| Error::invalid("stream size", id as usize))?;
        if size > self.data.len() {
            return Err(Error::invalid("stream size", id as usize));
        }
        let mini = n.kind != 5 && size < self.header.mini_cutoff as usize;
        let chain = if mini {
            self.chain(n.start, &self.minifat, (self.nodes[0].size as usize) / 64)?
        } else {
            self.chain(
                n.start,
                &self.fat,
                self.data.len() / self.header.sector_size - 1,
            )?
        };
        let unit = if mini { 64 } else { self.header.sector_size };
        if chain.len() != size.div_ceil(unit) {
            return Err(Error::invalid("stream chain length", id as usize));
        }
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut remaining = size;
        for sid in chain {
            let mut range = if mini {
                let logical = sid as usize * 64;
                let physical = *self
                    .mini_chain
                    .get(logical / self.header.sector_size)
                    .ok_or_else(|| Error::invalid("mini sector", logical))?;
                let start = self.sector_range(physical)?.start + logical % self.header.sector_size;
                start..start + 64
            } else {
                self.sector_range(sid)?
            };
            range.end = range.start + remaining.min(unit);
            remaining -= remaining.min(unit);
            if let Some(last) = ranges.last_mut().filter(|last| last.end == range.start) {
                last.end = range.end;
            } else {
                ranges.push(range);
            }
        }
        Ok(Stream {
            file: self,
            ranges,
            size,
        })
    }
}
/// Fragmented borrowed byte stream; extraction does not duplicate its payload.
pub struct Stream<'a> {
    file: &'a CompoundFile,
    ranges: Vec<Range<usize>>,
    pub size: usize,
}
impl<'a> Stream<'a> {
    pub fn chunks(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.ranges.iter().map(|r| &self.file.data[r.clone()])
    }
    pub fn to_vec(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.size);
        for b in self.chunks() {
            v.extend_from_slice(b);
        }
        v
    }
    pub fn reader(&self) -> StreamReader<'_, 'a> {
        StreamReader {
            stream: self,
            chunk: 0,
            offset: 0,
        }
    }
    pub fn copy_to(&self, mut out: impl Write) -> io::Result<u64> {
        for b in self.chunks() {
            out.write_all(b)?;
        }
        Ok(self.size as u64)
    }
    pub fn copy_range(&self, start: usize, len: usize, mut out: impl Write) -> Result<()> {
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::invalid("media range", start))?;
        if end > self.size {
            return Err(Error::invalid("media range", end));
        }
        let mut pos = 0;
        for b in self.chunks() {
            let next = pos + b.len();
            let left = start.max(pos);
            let right = end.min(next);
            if left < right {
                out.write_all(&b[left - pos..right - pos])?;
            }
            pos = next;
            if pos >= end {
                break;
            }
        }
        Ok(())
    }
    pub fn prefix(&self, len: usize) -> Vec<u8> {
        let mut data = vec![0; len.min(self.size)];
        // Reading from validated in-memory ranges cannot fail.
        self.reader()
            .read_exact(&mut data)
            .expect("validated stream ranges");
        data
    }
}
pub struct StreamReader<'s, 'a> {
    stream: &'s Stream<'a>,
    chunk: usize,
    offset: usize,
}
impl Read for StreamReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total = 0;
        while total < buf.len() {
            let Some(r) = self.stream.ranges.get(self.chunk) else {
                break;
            };
            let b = &self.stream.file.data[r.start + self.offset..r.end];
            let n = b.len().min(buf.len() - total);
            buf[total..total + n].copy_from_slice(&b[..n]);
            total += n;
            self.offset += n;
            if self.offset == r.len() {
                self.chunk += 1;
                self.offset = 0;
            }
        }
        Ok(total)
    }
}
