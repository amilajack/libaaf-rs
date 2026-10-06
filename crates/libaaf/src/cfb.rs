//! Checked Compound File Binary storage with bounded file reads or owned bytes.
use crate::{Error, Result, Uid, bytes::*};
use std::{
    borrow::Cow,
    collections::hash_map::RandomState,
    fs::File,
    hash::BuildHasher,
    io::{self, Read, Seek, SeekFrom, Write},
    ops::Range,
    path::Path,
    sync::Mutex,
};
const CACHE_SIZE: usize = 4 * 1024;
const COPY_SIZE: usize = 64 * 1024;

/// Native files retain an open handle and a bounded read cache. Owned input is
/// used by wasm and callers that require an immutable snapshot.
enum Backing {
    Owned(Vec<u8>),
    File { state: Mutex<FileCache>, len: usize },
}
struct FileCache {
    file: File,
    bytes: Vec<u8>,
    start: usize,
}
impl Backing {
    fn len(&self) -> usize {
        match self {
            Self::Owned(bytes) => bytes.len(),
            Self::File { len, .. } => *len,
        }
    }
    fn read_exact_at(&self, start: usize, out: &mut [u8]) -> io::Result<()> {
        if start
            .checked_add(out.len())
            .is_none_or(|end| end > self.len())
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "backing range",
            ));
        }
        match self {
            Self::Owned(bytes) => out.copy_from_slice(&bytes[start..start + out.len()]),
            Self::File { state, len } => {
                let mut cache = state
                    .lock()
                    .map_err(|_| io::Error::other("file cache poisoned"))?;
                let end = start + out.len();
                if start >= cache.start && end <= cache.start + cache.bytes.len() {
                    out.copy_from_slice(&cache.bytes[start - cache.start..end - cache.start]);
                } else if out.len() >= CACHE_SIZE {
                    cache.file.seek(SeekFrom::Start(start as u64))?;
                    cache.file.read_exact(out)?;
                } else {
                    let mut at = start;
                    let mut written = 0;
                    while written < out.len() {
                        let page = at / CACHE_SIZE * CACHE_SIZE;
                        let size = CACHE_SIZE.min(*len - page);
                        cache.bytes.clear();
                        cache.file.seek(SeekFrom::Start(page as u64))?;
                        let FileCache { file, bytes, .. } = &mut *cache;
                        bytes.resize(size, 0);
                        if let Err(error) = file.read_exact(bytes) {
                            bytes.clear();
                            return Err(error);
                        }
                        cache.start = page;
                        let count = (size - (at - page)).min(out.len() - written);
                        out[written..written + count]
                            .copy_from_slice(&cache.bytes[at - page..at - page + count]);
                        written += count;
                        at += count;
                    }
                }
            }
        }
        Ok(())
    }
    fn view(&self, range: Range<usize>) -> Result<Cow<'_, [u8]>> {
        match self {
            Self::Owned(bytes) => Ok(Cow::Borrowed(slice(bytes, range.start, range.len())?)),
            Self::File { .. } => {
                let mut bytes = vec![0; range.len()];
                self.read_exact_at(range.start, &mut bytes)?;
                Ok(Cow::Owned(bytes))
            }
        }
    }
}
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

fn folded_hash(hasher: &RandomState, name: &str) -> u64 {
    // CFB directory names fit in 31 UTF-16 units. Avoid a temporary heap
    // allocation for the usual ASCII names; arbitrary lookup strings remain valid.
    if name.is_ascii() && name.len() <= 64 {
        let mut folded = [0u8; 64];
        for (out, byte) in folded.iter_mut().zip(name.bytes()) {
            *out = byte.to_ascii_lowercase();
        }
        hasher.hash_one(&folded[..name.len()])
    } else {
        hasher.hash_one(name.to_lowercase().as_bytes())
    }
}

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
pub struct NodeRecord {
    pub id: u32,
    name: Range<usize>,
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
/// Borrowed directory entry; its name is stored in the compound file's string arena.
#[derive(Clone, Copy, Debug)]
pub struct Node<'a> {
    record: &'a NodeRecord,
    pub name: &'a str,
}
impl std::ops::Deref for Node<'_> {
    type Target = NodeRecord;
    fn deref(&self) -> &Self::Target {
        self.record
    }
}
/// Indexed directory records and their shared UTF-8 names.
#[derive(Default)]
pub struct Nodes {
    records: Vec<NodeRecord>,
    names: String,
}
impl Nodes {
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<Node<'_>> {
        let record = self.records.get(index)?;
        Some(Node {
            record,
            name: &self.names[record.name.clone()],
        })
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Node<'_>> + DoubleEndedIterator + '_ {
        self.records.iter().map(|record| Node {
            record,
            name: &self.names[record.name.clone()],
        })
    }
}
/// Owns either a native file handle with a bounded cache or an input byte buffer.
/// Files must not be modified while open; use `from_bytes` for a snapshot.
pub struct CompoundFile {
    data: Backing,
    pub header: Header,
    pub nodes: Nodes,
    pub fat: Vec<u32>,
    pub minifat: Vec<u32>,
    pub difat: Vec<u32>,
    mini_chain: Vec<u32>,
    // Hashes index the one owned name; collisions use full folded-name comparison.
    children: Vec<ChildIndex>,
    name_hasher: RandomState,
}
impl CompoundFile {
    pub(crate) fn file_size(&self) -> usize {
        self.data.len()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        let len =
            usize::try_from(file.metadata()?.len()).map_err(|_| Error::invalid("file size", 0))?;
        Self::parse(Backing::File {
            state: Mutex::new(FileCache {
                file,
                bytes: Vec::new(),
                start: 0,
            }),
            len,
        })
    }
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::parse(Backing::Owned(data))
    }
    fn parse(backing: Backing) -> Result<Self> {
        let data = backing.view(0..512)?;
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
        let sectors = backing.len() / header.sector_size;
        if sectors == 0 || !backing.len().is_multiple_of(header.sector_size) {
            return Err(Error::invalid("CFB file size", backing.len()));
        }
        let max = sectors - 1;
        if header.fat_sectors as usize > max
            || header.difat_sectors as usize > max
            || header.minifat_sectors as usize > max
        {
            return Err(Error::invalid("CFB sector count", 44));
        }
        let initial_difat = data[76..512].to_vec();
        drop(data);
        let mut file = Self {
            data: backing,
            header,
            nodes: Nodes::default(),
            fat: Vec::new(),
            minifat: Vec::new(),
            difat: Vec::new(),
            mini_chain: Vec::new(),
            children: Vec::new(),
            name_hasher: RandomState::new(),
        };
        for i in 0..109 {
            let id = u32_at(&initial_difat, i * 4)?;
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
        file.nodes
            .records
            .reserve_exact(dir.len() * file.header.sector_size / 128);
        let mut directory_sector = vec![0; file.header.sector_size];
        let mut name_bytes = 0;
        for (sector_index, &id) in dir.iter().enumerate() {
            file.data
                .read_exact_at(file.sector_range(id)?.start, &mut directory_sector)?;
            for (entry_index, b) in directory_sector.as_chunks::<128>().0.iter().enumerate() {
                if b[66] == 0 {
                    continue;
                }
                let nlen = u16_at(b, 64)? as usize;
                if !(2..=64).contains(&nlen) || !nlen.is_multiple_of(2) || u16_at(b, nlen - 2)? != 0
                {
                    return Err(Error::invalid(
                        "directory name",
                        sector_index * file.header.sector_size + entry_index * 128,
                    ));
                }
                let words = b[..nlen]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|v| u16::from_le_bytes(*v))
                    .take_while(|v| *v != 0);
                for ch in char::decode_utf16(words) {
                    name_bytes += ch
                        .map_err(|_| Error::invalid("UTF-16 encoding", 0))?
                        .len_utf8();
                }
            }
        }
        file.nodes.names.reserve_exact(name_bytes);
        for id in dir {
            file.data
                .read_exact_at(file.sector_range(id)?.start, &mut directory_sector)?;
            for b in directory_sector.as_chunks::<128>().0 {
                let i = file.nodes.len();
                let kind = b[66];
                let nlen = u16_at(b, 64)? as usize;
                let first_name = file.nodes.names.len();
                if kind != 0 {
                    if !(2..=64).contains(&nlen)
                        || !nlen.is_multiple_of(2)
                        || u16_at(b, nlen - 2)? != 0
                    {
                        return Err(Error::invalid("directory name", i * 128));
                    }
                    append_utf16(&b[..nlen], &mut file.nodes.names)?;
                }
                if !matches!(kind, 0 | 1 | 2 | 5) {
                    return Err(Error::invalid("directory type", i * 128 + 66));
                }
                let size = if major == 3 {
                    u32_at(b, 120)? as u64
                } else {
                    u64_at(b, 120)?
                };
                file.nodes.records.push(NodeRecord {
                    id: i as u32,
                    name: first_name..file.nodes.names.len(),
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
        }
        if file.nodes.records.first().is_none_or(|n| n.kind != 5) {
            return Err(Error::invalid("root directory", 0));
        }
        drop(directory_sector);
        file.children
            .reserve_exact(file.nodes.len().saturating_sub(1));
        let mut seen = vec![false; file.nodes.len()];
        seen[0] = true;
        let mut stack = vec![(file.nodes.get(0).unwrap().child, 0)];
        while let Some((id, parent)) = stack.pop() {
            if id == FREE {
                continue;
            }
            let n = file
                .nodes
                .records
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
                name_hash: folded_hash(&file.name_hasher, &file.nodes.names[n.name.clone()]),
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
                        file.nodes.get(a.node as usize).unwrap().name,
                        file.nodes.get(b.node as usize).unwrap().name,
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
        file.mini_chain = file.chain(file.nodes.get(0).unwrap().start, &file.fat, max)?;
        let root_size = usize::try_from(file.nodes.get(0).unwrap().size)
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
        if start
            .checked_add(self.header.sector_size)
            .is_none_or(|end| end > self.data.len())
        {
            return Err(Error::invalid("sector range", start));
        }
        Ok(start..start + self.header.sector_size)
    }
    pub fn sector(&self, id: u32) -> Result<Cow<'_, [u8]>> {
        self.data.view(self.sector_range(id)?)
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
        let key = (parent, folded_hash(&self.name_hasher, name));
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
            .find(|entry| {
                folded_cmp(self.nodes.get(entry.node as usize).unwrap().name, name).is_eq()
            })
            .map(|entry| entry.node)
    }
    pub fn node_by_path(&self, path: &str) -> Option<u32> {
        let mut id = 0;
        let mut parts = path.split(['/', '\\']).filter(|s| !s.is_empty()).peekable();
        if parts.peek().is_some_and(|v| {
            v.eq_ignore_ascii_case(self.nodes.get(0).unwrap().name)
                || v.eq_ignore_ascii_case("Root")
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
                names.push(n.name);
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
                first: 0..0,
                rest: Vec::new(),
                size: 0,
            });
        }
        let size =
            usize::try_from(n.size).map_err(|_| Error::invalid("stream size", id as usize))?;
        if size > self.data.len() {
            return Err(Error::invalid("stream size", id as usize));
        }
        let mini = n.kind != 5 && size < self.header.mini_cutoff as usize;
        let (table, max) = if mini {
            (
                &self.minifat,
                (self.nodes.get(0).unwrap().size as usize) / 64,
            )
        } else {
            (&self.fat, self.data.len() / self.header.sector_size - 1)
        };
        let unit = if mini { 64 } else { self.header.sector_size };
        let expected = size.div_ceil(unit);
        if expected > max {
            return Err(Error::invalid("stream chain length", id as usize));
        }
        let mut first: Range<usize> = 0..0;
        let mut rest: Vec<Range<usize>> = Vec::new();
        let mut remaining = size;
        let mut sid = n.start;
        for _ in 0..expected {
            if sid as usize >= max || matches!(sid, FREE | END | FAT | DIF) {
                return Err(Error::invalid("stream sector", sid as usize));
            }
            let next = *table
                .get(sid as usize)
                .ok_or_else(|| Error::invalid("allocation table", sid as usize))?;
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
            let last = rest.last_mut().unwrap_or(&mut first);
            if last.start == last.end {
                *last = range;
            } else if last.end == range.start {
                last.end = range.end;
            } else {
                rest.push(range);
            }
            sid = next;
        }
        // This also rejects cyclic chains after at most the declared stream size.
        if sid != END {
            return Err(Error::invalid("stream chain length", id as usize));
        }
        Ok(Stream {
            file: self,
            first,
            rest,
            size,
        })
    }
}
/// Fragmented stream borrowing its backing store; extraction never buffers the full payload.
pub struct Stream<'a> {
    file: &'a CompoundFile,
    first: Range<usize>,
    rest: Vec<Range<usize>>,
    pub size: usize,
}
impl<'a> Stream<'a> {
    fn ranges(&self) -> impl Iterator<Item = &Range<usize>> {
        (!self.first.is_empty())
            .then_some(&self.first)
            .into_iter()
            .chain(self.rest.iter())
    }
    fn range(&self, index: usize) -> Option<&Range<usize>> {
        if index == 0 {
            (!self.first.is_empty()).then_some(&self.first)
        } else {
            self.rest.get(index - 1)
        }
    }

    /// Reads bounded chunks; owned backing returns borrowed slices.
    pub fn chunks(&self) -> impl Iterator<Item = Result<Cow<'_, [u8]>>> + '_ {
        self.ranges()
            .flat_map(|r| {
                (r.start..r.end)
                    .step_by(COPY_SIZE)
                    .map(move |start| start..r.end.min(start.saturating_add(COPY_SIZE)))
            })
            .map(|range| self.file.data.view(range))
    }
    pub fn to_vec(&self) -> Result<Vec<u8>> {
        self.prefix(self.size)
    }
    pub fn reader(&self) -> StreamReader<'_, 'a> {
        StreamReader {
            stream: self,
            chunk: 0,
            offset: 0,
        }
    }
    pub fn copy_to(&self, mut out: impl Write) -> io::Result<u64> {
        io::copy(&mut self.reader(), &mut out)
    }
    pub fn copy_range(&self, start: usize, len: usize, mut out: impl Write) -> Result<()> {
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::invalid("media range", start))?;
        if end > self.size {
            return Err(Error::invalid("media range", end));
        }
        let mut pos = 0;
        for range in self.ranges() {
            let next = pos + range.len();
            let left = start.max(pos);
            let right = end.min(next);
            if left < right {
                let physical = range.start + left - pos;
                for start in (physical..physical + right - left).step_by(COPY_SIZE) {
                    let end = (start + COPY_SIZE).min(physical + right - left);
                    out.write_all(&self.file.data.view(start..end)?)?;
                }
            }
            pos = next;
            if pos >= end {
                break;
            }
        }
        Ok(())
    }
    pub fn prefix(&self, len: usize) -> Result<Vec<u8>> {
        let mut data = vec![0; len.min(self.size)];
        self.reader().read_exact(&mut data)?;
        Ok(data)
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
            let Some(r) = self.stream.range(self.chunk) else {
                break;
            };
            let start = r.start + self.offset;
            let n = (r.end - start).min(buf.len() - total);
            if let Err(error) = self
                .stream
                .file
                .data
                .read_exact_at(start, &mut buf[total..total + n])
            {
                // Preserve successfully consumed ranges when a later file read fails.
                return if total == 0 { Err(error) } else { Ok(total) };
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_reads_report_truncation() {
        let path = std::env::temp_dir().join(format!("libaaf-backing-{}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let expected: Vec<_> = (0..CACHE_SIZE * 3).map(|i| (i % 251) as u8).collect();
        file.write_all(&expected).unwrap();
        let backing = Backing::File {
            state: Mutex::new(FileCache {
                file,
                bytes: Vec::new(),
                start: 0,
            }),
            len: CACHE_SIZE * 3,
        };
        for (start, len) in [
            (0, 8),
            (CACHE_SIZE - 7, 31),
            (CACHE_SIZE + 3, 123),
            (5, CACHE_SIZE * 2),
        ] {
            let mut actual = vec![0; len];
            backing.read_exact_at(start, &mut actual).unwrap();
            assert_eq!(actual, expected[start..start + len]);
            assert!(backing.state_for_test().lock().unwrap().bytes.len() <= CACHE_SIZE);
        }
        std::thread::scope(|scope| {
            for start in [1, CACHE_SIZE - 3, CACHE_SIZE * 2] {
                let backing = &backing;
                let expected = &expected;
                scope.spawn(move || {
                    let mut actual = [0; 17];
                    backing.read_exact_at(start, &mut actual).unwrap();
                    assert_eq!(actual, expected[start..start + 17]);
                });
            }
        });
        backing
            .state_for_test()
            .lock()
            .unwrap()
            .file
            .set_len(0)
            .unwrap();
        assert_eq!(
            backing
                .read_exact_at(0, &mut vec![0; CACHE_SIZE * 2])
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(backing.read_exact_at(usize::MAX, &mut [0; 8]).is_err());
        drop(backing);
        std::fs::remove_file(path).unwrap();
    }
    impl Backing {
        fn state_for_test(&self) -> &Mutex<FileCache> {
            match self {
                Self::File { state, .. } => state,
                _ => unreachable!(),
            }
        }
    }
}
