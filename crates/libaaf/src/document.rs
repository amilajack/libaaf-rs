//! AAF metadata dictionary, object arena, and indexed references.
use crate::{Diagnostic, Error, Rational, Result, Uid, bytes::*, cfb::CompoundFile, defs::*};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
pub struct ReaderOptions {
    pub media_location: Option<PathBuf>,
    pub recover_protools_fades: bool,
    pub remove_sample_accurate_edits: bool,
    pub mobid_filenames: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjectId(pub u32);
#[derive(Debug)]
struct PropertyRecord {
    id: u16,
    stored_form: u16,
    byte_start: usize,
    reference_start: usize,
    reference_count: u32,
    byte_len: u16,
}
impl PropertyRecord {
    fn from_words(w: &[u64; 4]) -> Self {
        Self {
            byte_start: w[0] as usize,
            reference_start: w[1] as usize,
            id: w[2] as u16,
            stored_form: (w[2] >> 16) as u16,
            byte_len: (w[2] >> 32) as u16,
            reference_count: w[3] as u32,
        }
    }
    fn words(&self) -> [u64; 4] {
        [
            self.byte_start as u64,
            self.reference_start as u64,
            u64::from(self.id)
                | (u64::from(self.stored_form) << 16)
                | (u64::from(self.byte_len) << 32),
            u64::from(self.reference_count),
        ]
    }

    fn bytes(&self) -> Range<usize> {
        self.byte_start..self.byte_start + usize::from(self.byte_len)
    }
    fn references(&self) -> Range<usize> {
        self.reference_start..self.reference_start + self.reference_count as usize
    }
}
#[derive(Debug)]
struct ObjectRecord {
    data: Range<usize>,
    property_start: usize,
    node: u32,
    property_count: u16,
}
impl ObjectRecord {
    fn from_words(w: &[u64; 4]) -> Self {
        Self {
            data: w[0] as usize..(w[0] + w[1]) as usize,
            property_start: w[2] as usize,
            node: w[3] as u32,
            property_count: (w[3] >> 32) as u16,
        }
    }
    fn words(&self) -> [u64; 4] {
        [
            self.data.start as u64,
            self.data.len() as u64,
            self.property_start as u64,
            u64::from(self.node) | (u64::from(self.property_count) << 32),
        ]
    }

    fn properties(&self) -> Range<usize> {
        self.property_start..self.property_start + usize::from(self.property_count)
    }
}
/// Borrowed property view. Original byte offsets remain relative to its object's stream.
#[derive(Clone, Debug)]
pub struct Property<'a> {
    pub id: u16,
    pub stored_form: u16,
    pub bytes: Range<usize>,
    pub references: &'a [ObjectId],
}
/// Borrowed object view backed by the document's immutable metadata arenas.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    pub id: ObjectId,
    pub node: u32,
    pub class: Uid,
    pub parent: Option<ObjectId>,
    properties: &'a [[u64; 4]],
    data: &'a [u8],
    references: &'a [ObjectId],
}
impl<'a> Object<'a> {
    pub fn properties(
        &self,
    ) -> impl ExactSizeIterator<Item = Property<'a>> + DoubleEndedIterator + 'a {
        let references = self.references;
        self.properties.iter().map(move |words| {
            let p = PropertyRecord::from_words(words);
            Property {
                id: p.id,
                stored_form: p.stored_form,
                bytes: p.bytes(),
                references: &references[p.references()],
            }
        })
    }
    pub fn property(&self, pid: u16) -> Option<Property<'a>> {
        self.properties().find(|p| p.id == pid)
    }
    pub fn raw(&self, pid: u16) -> Option<&'a [u8]> {
        self.properties
            .iter()
            .find(|w| w[2] as u16 == pid)
            .map(|w| &self.data[PropertyRecord::from_words(w).bytes()])
    }
    pub fn references(&self, pid: u16) -> &'a [ObjectId] {
        self.properties
            .iter()
            .find(|w| w[2] as u16 == pid)
            .map(|w| &self.references[PropertyRecord::from_words(w).references()])
            .unwrap_or(&[])
    }
    pub fn reference(&self, pid: u16) -> Option<ObjectId> {
        self.references(pid).first().copied()
    }
    pub fn string(&self, pid: u16) -> Option<String> {
        utf16(self.raw(pid)?).ok()
    }
    pub fn uid(&self, pid: u16) -> Option<Uid> {
        Uid::from_bytes(self.raw(pid)?)
    }
    pub fn u16(&self, pid: u16) -> Option<u16> {
        u16_at(self.raw(pid)?, 0).ok()
    }
    pub fn u32(&self, pid: u16) -> Option<u32> {
        u32_at(self.raw(pid)?, 0).ok()
    }
    pub fn i64(&self, pid: u16) -> Option<i64> {
        u64_at(self.raw(pid)?, 0).ok().map(|v| v as i64)
    }
    pub fn rational(&self, pid: u16) -> Option<Rational> {
        Rational::from_bytes(self.raw(pid)?).ok()
    }
    pub fn indirect(&self, pid: u16) -> Option<(Uid, &'a [u8])> {
        let b = self.raw(pid)?;
        Some((Uid::from_bytes(b.get(1..17)?)?, b.get(17..)?))
    }
}
#[derive(Debug, Clone)]
pub struct PropertyDefinition {
    pub id: u16,
    pub name: String,
    pub required: bool,
    pub data_type: Uid,
    pub custom: bool,
}
#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub id: Uid,
    pub name: String,
    pub parent: Option<Uid>,
    pub concrete: bool,
    pub custom: bool,
    pub properties: Vec<PropertyDefinition>,
}
pub struct Document {
    pub cfb: CompoundFile,
    pub path: Option<PathBuf>,
    // One allocation contains four-word object records, four-word property
    // records, then original bytes padded to a u64 boundary. Records are written
    // only from checked usize/u16/u32 values; no wire bytes are cast to structs.
    metadata: Box<[u64]>,
    object_count: usize,
    property_words: usize,
    data_words: usize,
    references: Box<[ObjectId]>,
    pub classes: Vec<ClassDefinition>,
    pub root: ObjectId,
    pub header: ObjectId,
    pub dictionary: ObjectId,
    pub content: ObjectId,
    pub identification: Option<ObjectId>,
    pub options: ReaderOptions,
    pub diagnostics: Vec<Diagnostic>,
    node_objects: Vec<Option<ObjectId>>,
    pub(crate) mobs: HashMap<Vec<u8>, ObjectId>,
    identifiers: HashMap<(u16, Vec<u8>), ObjectId>,
    class_index: HashMap<Uid, usize>,
}
impl Document {
    pub fn open(path: impl AsRef<Path>, options: ReaderOptions) -> Result<Self> {
        let path = path.as_ref().canonicalize()?;
        let cfb = CompoundFile::open(&path)?;
        Self::decode(cfb, Some(path), options)
    }
    pub fn from_bytes(bytes: Vec<u8>, options: ReaderOptions) -> Result<Self> {
        Self::decode(CompoundFile::from_bytes(bytes)?, None, options)
    }
    fn decode(cfb: CompoundFile, path: Option<PathBuf>, options: ReaderOptions) -> Result<Self> {
        let storage_count = cfb.nodes.iter().filter(|n| matches!(n.kind, 1 | 5)).count();
        let mut node_objects = vec![None; cfb.nodes.len()];
        let mut streams = Vec::with_capacity(storage_count);
        let mut data_size = 0usize;
        let mut property_count = 0usize;
        for n in cfb.nodes.iter() {
            if n.kind != 1 && n.kind != 5 {
                continue;
            }
            let Some(node) = cfb.child(n.id, "properties") else {
                continue;
            };
            let size = usize::try_from(cfb.nodes.get(node as usize).unwrap().size)
                .map_err(|_| Error::invalid("property stream size", node as usize))?;
            data_size = data_size
                .checked_add(size)
                .ok_or_else(|| Error::invalid("metadata size", node as usize))?;
            if data_size > cfb.file_size() {
                return Err(Error::invalid("metadata stream sizes", node as usize));
            }
            let mut header = [0; 4];
            cfb.stream(node)?.reader().read_exact(&mut header)?;
            let count = u16_at(&header, 2)?;
            if 4 + usize::from(count) * 6 > size {
                return Err(Error::invalid("property table size", node as usize));
            }
            property_count = property_count
                .checked_add(usize::from(count))
                .ok_or_else(|| Error::invalid("property count", node as usize))?;
            streams.push((n.id, node, size, count));
        }
        let object_count = streams.len();
        let property_words = object_count
            .checked_mul(4)
            .ok_or_else(|| Error::invalid("object arena size", 0))?;
        let data_words = property_count
            .checked_mul(4)
            .and_then(|n| n.checked_add(property_words))
            .ok_or_else(|| Error::invalid("property arena size", 0))?;
        let word_count = data_words
            .checked_add(data_size.div_ceil(8))
            .ok_or_else(|| Error::invalid("metadata arena size", 0))?;
        let mut metadata = Vec::new();
        metadata
            .try_reserve_exact(word_count)
            .map_err(|_| Error::Limit {
                context: "metadata allocation",
                limit: word_count,
            })?;
        metadata.resize(word_count, 0u64);
        let mut data_position = 0;
        let mut property_position = 0;
        for (object_index, (node, stream, size, count)) in streams.into_iter().enumerate() {
            let id = ObjectId(
                u32::try_from(object_index)
                    .map_err(|_| Error::invalid("object ID", object_index))?,
            );
            node_objects[node as usize] = Some(id);
            let (records, raw_words) = metadata.split_at_mut(data_words);
            let (object_records, property_records) = records.split_at_mut(property_words);
            // Safe u64 -> u8 casts: source is initialized, destination alignment
            // is 1, and both types admit every bit pattern. Raw bytes are separate
            // from record words, so mutable borrows cannot overlap.
            let raw = bytemuck::cast_slice_mut::<u64, u8>(raw_words);
            let data_end = data_position + size; // Checked total before allocation.
            let data = &mut raw[data_position..data_end];
            cfb.stream(stream)?.reader().read_exact(data)?;
            if !matches!(data[0], 0x4c | 0x55) {
                return Err(Error::Unsupported(format!(
                    "property byte order 0x{:02x}",
                    data[0]
                )));
            }
            if u16_at(data, 2)? != count {
                return Err(Error::invalid(
                    "property header changed while reading",
                    node as usize,
                ));
            }
            let object = ObjectRecord {
                node,
                property_start: property_position,
                property_count: count,
                data: data_position..data_end,
            };
            object_records[object_index * 4..object_index * 4 + 4].copy_from_slice(&object.words());
            let mut offset = 4 + usize::from(count) * 6;
            let mut seen = HashSet::new();
            for i in 0..usize::from(count) {
                let at = 4 + i * 6;
                let pid = u16_at(data, at)?;
                let form = u16_at(data, at + 2)?;
                let len = u16_at(data, at + 4)?;
                let duplicate = if count <= 16 {
                    property_records[object.property_start * 4..property_position * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|w| w[2] as u16 == pid)
                } else {
                    !seen.insert(pid)
                };
                if duplicate {
                    return Err(Error::invalid("duplicate property ID", at));
                }
                slice(data, offset, usize::from(len))?;
                let p = PropertyRecord {
                    id: pid,
                    stored_form: form,
                    byte_start: offset,
                    byte_len: len,
                    reference_start: 0,
                    reference_count: 0,
                };
                property_records[property_position * 4..property_position * 4 + 4]
                    .copy_from_slice(&p.words());
                property_position += 1;
                offset += usize::from(len);
            }
            data_position = data_end;
        }
        let root = node_objects[0].ok_or_else(|| Error::Missing {
            context: "AAF root",
            name: "properties".into(),
        })?;
        let mut diagnostics = Vec::new();
        let mut identifiers = HashMap::new();
        let mut references = Vec::new();
        for object_index in 0..object_count {
            let obj = ObjectRecord::from_words(
                &metadata[..property_words].as_chunks::<4>().0[object_index],
            );
            for property_index in obj.properties() {
                let at = property_words + property_index * 4;
                let mut p = PropertyRecord::from_words(metadata[at..at + 4].try_into().unwrap());
                if !matches!(p.stored_form, 0x22 | 0x32 | 0x3a) {
                    continue;
                }
                let raw = bytemuck::cast_slice::<u64, u8>(&metadata[data_words..]);
                let name = utf16(&raw[obj.data.clone()][p.bytes()])?;
                let first_reference = references.len();
                if p.stored_form == 0x22 {
                    if let Some(id) = cfb
                        .child(obj.node, &name)
                        .and_then(|n| node_objects[n as usize])
                    {
                        references.push(id);
                    } else {
                        diagnostics.push(Diagnostic {
                            object: Some(ObjectId(object_index as u32)),
                            message: format!("missing strong reference {name}"),
                        });
                    }
                } else {
                    let index = cfb
                        .child(obj.node, &format!("{name} index"))
                        .ok_or_else(|| Error::Missing {
                            context: "strong reference index",
                            name: name.clone(),
                        })?;
                    let b = cfb.stream(index)?.to_vec()?;
                    let count = u32_at(&b, 0)? as usize;
                    let set = p.stored_form == 0x3a;
                    let (header, stride, pid, keylen) = if set {
                        slice(&b, 0, 15)?;
                        (15, 8 + b[14] as usize, u16_at(&b, 12)?, b[14] as usize)
                    } else {
                        (12, 4, 0, 0)
                    };
                    let bytes = count
                        .checked_mul(stride)
                        .ok_or_else(|| Error::invalid("reference count", 0))?;
                    slice(&b, header, bytes)?;
                    references.reserve(count);
                    for i in 0..count {
                        let at = header + i * stride;
                        let key = u32_at(&b, at)?;
                        let target = format!("{name}{{{key:x}}}");
                        if let Some(id) = cfb
                            .child(obj.node, &target)
                            .and_then(|n| node_objects[n as usize])
                        {
                            references.push(id);
                            if set {
                                identifiers.insert((pid, b[at + 8..at + 8 + keylen].to_vec()), id);
                            }
                        } else {
                            diagnostics.push(Diagnostic {
                                object: Some(ObjectId(object_index as u32)),
                                message: format!("missing set/vector member {target}"),
                            });
                        }
                    }
                    if set {
                        references[first_reference..].reverse();
                    }
                }
                p.reference_start = first_reference;
                p.reference_count = u32::try_from(references.len() - first_reference)
                    .map_err(|_| Error::invalid("reference count", obj.node as usize))?;
                metadata[at..at + 4].copy_from_slice(&p.words());
            }
        }
        let mut doc = Self {
            cfb,
            path,
            metadata: metadata.into_boxed_slice(),
            object_count,
            property_words,
            data_words,
            references: references.into_boxed_slice(),
            classes: Vec::new(),
            root,
            header: root,
            dictionary: root,
            content: root,
            identification: None,
            options,
            diagnostics,
            node_objects,
            mobs: HashMap::new(),
            identifiers,
            class_index: HashMap::new(),
        };
        doc.header = doc.required_ref(root, PID_Root_Header)?;
        doc.dictionary = doc.required_ref(doc.header, PID_Header_Dictionary)?;
        doc.content = doc.required_ref(doc.header, PID_Header_Content)?;
        doc.identification = doc
            .object(doc.header)
            .reference(PID_Header_IdentificationList);
        for id in doc
            .object(doc.content)
            .references(PID_ContentStorage_Mobs)
            .to_vec()
        {
            if let Some(key) = doc.object(id).raw(PID_Mob_MobID) {
                doc.mobs.insert(key.to_vec(), id);
            }
        }
        doc.decode_dictionary()?;
        Ok(doc)
    }
    fn required_ref(&self, id: ObjectId, pid: u16) -> Result<ObjectId> {
        self.object(id)
            .reference(pid)
            .ok_or_else(|| Error::Missing {
                context: "required AAF reference",
                name: format!("{pid:04x}"),
            })
    }
    /// Iterate borrowed views without allocating per-object wrappers.
    ///
    /// ```no_run
    /// # use libaaf::{Document, ReaderOptions};
    /// let doc = Document::open("session.aaf", ReaderOptions::default())?;
    /// for object in doc.objects() {
    ///     for property in object.properties() {
    ///         let bytes = object.raw(property.id).unwrap();
    ///         println!("{:04x}: {} bytes", property.id, bytes.len());
    ///     }
    /// }
    /// # Ok::<(), libaaf::Error>(())
    /// ```
    pub fn objects(&self) -> impl ExactSizeIterator<Item = Object<'_>> + '_ {
        (0..self.object_count).map(|index| self.object(ObjectId(index as u32)))
    }
    pub fn object(&self, id: ObjectId) -> Object<'_> {
        self.get_object(id)
            .expect("object ID belongs to this document")
    }
    pub fn get_object(&self, id: ObjectId) -> Option<Object<'_>> {
        if id.0 as usize >= self.object_count {
            return None;
        }
        let o = ObjectRecord::from_words(
            &self.metadata[..self.property_words].as_chunks::<4>().0[id.0 as usize],
        );
        let properties = &self.metadata[self.property_words..self.data_words]
            .as_chunks::<4>()
            .0[o.properties()];
        let raw = bytemuck::cast_slice::<u64, u8>(&self.metadata[self.data_words..]);
        Some(Object {
            id,
            node: o.node,
            class: self.cfb.nodes.get(o.node as usize).unwrap().class,
            parent: self
                .cfb
                .nodes
                .get(o.node as usize)
                .unwrap()
                .parent
                .and_then(|node| self.node_objects[node as usize]),
            properties,
            data: &raw[o.data],
            references: &self.references,
        })
    }
    pub fn object_by_node(&self, id: u32) -> Option<Object<'_>> {
        self.node_objects
            .get(id as usize)
            .copied()
            .flatten()
            .map(|id| self.object(id))
    }
    pub fn weak_target(&self, b: &[u8]) -> Option<ObjectId> {
        let pid = u16_at(b, 2).ok()?;
        let len = *b.get(4)? as usize;
        self.identifiers
            .get(&(pid, b.get(5..5 + len)?.to_vec()))
            .copied()
    }
    pub fn weak_uid(&self, id: ObjectId, pid: u16) -> Option<Uid> {
        let b = self.object(id).raw(pid)?;
        let target = self.weak_target(b)?;
        let o = self.object(target);
        o.uid(PID_DefinitionObject_Identification)
            .or_else(|| o.uid(PID_MetaDefinition_Identification))
    }
    pub fn mob(&self, id: &[u8]) -> Option<ObjectId> {
        self.mobs.get(id).copied()
    }
    pub fn class(&self, id: Uid) -> Option<&ClassDefinition> {
        self.class_index.get(&id).map(|&i| &self.classes[i])
    }
    pub fn class_name(&self, id: Uid) -> String {
        id.name()
            .map(str::to_owned)
            .or_else(|| self.class(id).map(|c| c.name.clone()))
            .unwrap_or_else(|| id.to_string())
    }
    pub fn property_name(&self, pid: u16) -> String {
        PROPERTY_NAMES
            .iter()
            .find(|(id, _)| *id == pid)
            .map(|(_, s)| s.to_string())
            .or_else(|| {
                self.classes
                    .iter()
                    .flat_map(|c| &c.properties)
                    .find(|p| p.id == pid)
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| format!("0x{pid:04x}"))
    }
    pub fn property_id(&self, name: &str) -> Option<u16> {
        self.classes
            .iter()
            .flat_map(|c| &c.properties)
            .find(|p| p.name == name)
            .map(|p| p.id)
    }
    pub fn named_raw(&self, id: ObjectId, name: &str) -> Option<&[u8]> {
        self.object(id).raw(self.property_id(name)?)
    }
    pub fn named_reference(&self, id: ObjectId, name: &str) -> Option<ObjectId> {
        self.object(id).reference(self.property_id(name)?)
    }
    pub fn named_references(&self, id: ObjectId, name: &str) -> &[ObjectId] {
        self.property_id(name)
            .map(|pid| self.object(id).references(pid))
            .unwrap_or(&[])
    }
    pub fn inherits(&self, class: Uid, ancestor: Uid) -> bool {
        let mut c = Some(class);
        for _ in 0..self.classes.len() + 1 {
            let Some(id) = c else {
                return false;
            };
            if id == ancestor {
                return true;
            }
            c = self.class(id).and_then(|c| c.parent);
        }
        false
    }
    pub fn tagged(&self, id: ObjectId, pid: u16) -> Vec<(String, String)> {
        self.object(id)
            .references(pid)
            .iter()
            .filter_map(|&id| {
                let o = self.object(id);
                let name = o.string(PID_TaggedValue_Name)?;
                let (typ, b) = o.indirect(PID_TaggedValue_Value)?;
                let value = if typ == AAFTypeID_String {
                    utf16(b).ok()?
                } else if typ == AAFTypeID_Int32 {
                    (u32_at(b, 0).ok()? as i32).to_string()
                } else {
                    return None;
                };
                Some((name, value))
            })
            .collect()
    }
    pub fn tagged_named(&self, id: ObjectId, property: &str, name: &str) -> Option<String> {
        self.tagged(id, self.property_id(property)?)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
    pub fn stream_property(
        &self,
        id: ObjectId,
        pid: u16,
    ) -> Result<Option<crate::cfb::Stream<'_>>> {
        let Some(b) = self.object(id).raw(pid) else {
            return Ok(None);
        };
        let name = utf16(
            b.get(1..)
                .ok_or_else(|| Error::invalid("stream property", 0))?,
        )?;
        let node = self
            .cfb
            .child(self.object(id).node, &name)
            .ok_or(Error::Missing {
                context: "data stream",
                name,
            })?;
        Ok(Some(self.cfb.stream(node)?))
    }
    fn decode_dictionary(&mut self) -> Result<()> {
        for &(id, parent, concrete) in STANDARD_CLASSES {
            self.class_index.insert(id, self.classes.len());
            self.classes.push(ClassDefinition {
                id,
                name: id
                    .name()
                    .unwrap_or("unknown")
                    .trim_start_matches("AAFClassID_")
                    .into(),
                parent,
                concrete,
                custom: false,
                properties: Vec::new(),
            });
        }
        for &(class, id, required) in STANDARD_PROPERTIES {
            if let Some(&i) = self.class_index.get(&class) {
                self.classes[i].properties.push(PropertyDefinition {
                    id,
                    name: PROPERTY_NAMES
                        .iter()
                        .find(|(pid, _)| *pid == id)
                        .map(|(_, n)| n.rsplit('_').next().unwrap_or(n))
                        .unwrap_or("unknown")
                        .into(),
                    required,
                    data_type: Uid::default(),
                    custom: false,
                });
            }
        }
        let md = self.required_ref(self.root, PID_Root_MetaDictionary)?;
        let class_ids = self
            .object(md)
            .references(PID_MetaDictionary_ClassDefinitions)
            .to_vec();
        let mut order = Vec::new();
        for id in class_ids {
            let mut stack = vec![(id, false)];
            let mut active = HashSet::new();
            while let Some((id, leave)) = stack.pop() {
                if leave {
                    active.remove(&id);
                    order.push(id);
                    continue;
                }
                if !active.insert(id) {
                    return Err(Error::Cycle {
                        context: "class inheritance",
                        id: id.0,
                    });
                }
                if active.len() > 256 {
                    return Err(Error::Limit {
                        context: "class inheritance",
                        limit: 256,
                    });
                }
                stack.push((id, true));
                if let Some(parent) = self
                    .object(id)
                    .raw(PID_ClassDefinition_ParentClass)
                    .and_then(|b| self.weak_target(b))
                    .filter(|p| *p != id)
                {
                    stack.push((parent, false));
                }
            }
        }
        for id in order {
            let o = self.object(id);
            let uid = o
                .uid(PID_MetaDefinition_Identification)
                .ok_or_else(|| Error::invalid("class identification", 0))?;
            let name = o
                .string(PID_MetaDefinition_Name)
                .unwrap_or_else(|| uid.to_string());
            let parent = self
                .weak_uid(id, PID_ClassDefinition_ParentClass)
                .filter(|p| *p != uid);
            let concrete = o
                .raw(PID_ClassDefinition_IsConcrete)
                .is_some_and(|b| b.first() == Some(&1));
            let props = o.references(PID_ClassDefinition_Properties).to_vec();
            let index = if let Some(&i) = self.class_index.get(&uid) {
                self.classes[i].name = name;
                i
            } else {
                let i = self.classes.len();
                self.class_index.insert(uid, i);
                self.classes.push(ClassDefinition {
                    id: uid,
                    name,
                    parent,
                    concrete,
                    custom: true,
                    properties: Vec::new(),
                });
                i
            };
            for pid in props {
                let o = self.object(pid);
                let Some(local) = o.u16(PID_PropertyDefinition_LocalIdentification) else {
                    continue;
                };
                if self.classes[index].properties.iter().any(|p| p.id == local) {
                    continue;
                }
                let prop = PropertyDefinition {
                    id: local,
                    name: o.string(PID_MetaDefinition_Name).unwrap_or_default(),
                    required: o
                        .raw(PID_PropertyDefinition_IsOptional)
                        .is_some_and(|b| b.first() == Some(&0)),
                    data_type: o
                        .uid(PID_PropertyDefinition_Type)
                        .or_else(|| self.weak_uid(pid, PID_PropertyDefinition_Type))
                        .unwrap_or_default(),
                    custom: true,
                };
                let unresolved_type = o.raw(PID_PropertyDefinition_Type).is_none_or(|b| {
                    let Some(&len) = b.get(4) else {
                        return true;
                    };
                    let Some(ident) = b.get(5..5 + len as usize) else {
                        return true;
                    };
                    !self
                        .object(md)
                        .references(PID_MetaDictionary_TypeDefinitions)
                        .iter()
                        .any(|&id| {
                            self.object(id)
                                .raw(PID_MetaDefinition_Identification)
                                .is_some_and(|key| key.starts_with(ident))
                        })
                });
                self.classes[index].properties.push(prop);
                // Pinned LibAAF stops registering this class at a missing type.
                // The complete definition objects and unknown property bytes remain inspectable.
                if unresolved_type {
                    break;
                }
            }
        }
        for c in &self.classes {
            let mut id = Some(c.id);
            let mut seen = HashSet::new();
            while let Some(uid) = id {
                if !seen.insert(uid) {
                    return Err(Error::Cycle {
                        context: "class inheritance",
                        id: 0,
                    });
                }
                id = self.class(uid).and_then(|c| c.parent);
            }
        }
        Ok(())
    }
    pub fn composition(&self) -> Result<crate::Composition> {
        crate::timeline::parse(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_metadata_preserves_full_field_widths() {
        let property = PropertyRecord {
            id: u16::MAX,
            stored_form: u16::MAX - 1,
            byte_start: usize::MAX / 2,
            byte_len: u16::MAX,
            reference_start: 0,
            reference_count: u32::MAX,
        };
        let decoded = PropertyRecord::from_words(&property.words());
        assert_eq!(decoded.id, u16::MAX);
        assert_eq!(decoded.stored_form, u16::MAX - 1);
        assert_eq!(decoded.bytes(), property.bytes());
        assert_eq!(decoded.references(), 0..u32::MAX as usize);
        let object = ObjectRecord {
            data: usize::MAX / 2..usize::MAX / 2 + 17,
            property_start: 0,
            property_count: u16::MAX,
            node: u32::MAX - 1,
        };
        let decoded = ObjectRecord::from_words(&object.words());
        assert_eq!(decoded.data, object.data);
        assert_eq!(decoded.node, u32::MAX - 1);
        assert_eq!(decoded.properties(), 0..u16::MAX as usize);
    }
}
