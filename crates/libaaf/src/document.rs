//! AAF metadata dictionary, object arena, and indexed references.
use crate::{Diagnostic, Error, Rational, Result, Uid, bytes::*, cfb::CompoundFile, defs::*};
use std::{
    collections::{HashMap, HashSet},
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
pub struct Property {
    pub id: u16,
    pub stored_form: u16,
    pub bytes: Range<usize>,
    pub references: Vec<ObjectId>,
}
#[derive(Debug)]
pub struct Object {
    pub id: ObjectId,
    pub node: u32,
    pub class: Uid,
    pub parent: Option<ObjectId>,
    pub properties: Vec<Property>,
    data: Vec<u8>,
}
impl Object {
    pub fn property(&self, pid: u16) -> Option<&Property> {
        self.properties.iter().find(|p| p.id == pid)
    }
    pub fn raw(&self, pid: u16) -> Option<&[u8]> {
        self.property(pid).map(|p| &self.data[p.bytes.clone()])
    }
    pub fn references(&self, pid: u16) -> &[ObjectId] {
        self.property(pid)
            .map(|p| p.references.as_slice())
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
    pub fn indirect(&self, pid: u16) -> Option<(Uid, &[u8])> {
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
    pub objects: Vec<Object>,
    pub classes: Vec<ClassDefinition>,
    pub root: ObjectId,
    pub header: ObjectId,
    pub dictionary: ObjectId,
    pub content: ObjectId,
    pub identification: Option<ObjectId>,
    pub options: ReaderOptions,
    pub diagnostics: Vec<Diagnostic>,
    node_objects: HashMap<u32, ObjectId>,
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
        let mut objects = Vec::with_capacity(storage_count);
        let mut node_objects = HashMap::with_capacity(storage_count);
        for n in &cfb.nodes {
            if n.kind != 1 && n.kind != 5 {
                continue;
            }
            let Some(properties) = cfb.child(n.id, "properties") else {
                continue;
            };
            let data = cfb.stream(properties)?.to_vec();
            slice(&data, 0, 4)?;
            if !matches!(data[0], 0x4c | 0x55) {
                return Err(Error::Unsupported(format!(
                    "property byte order 0x{:02x}",
                    data[0]
                )));
            }
            let count = u16_at(&data, 2)? as usize;
            let mut offset = 4 + count * 6;
            slice(&data, 4, count * 6)?;
            let id = ObjectId(objects.len() as u32);
            node_objects.insert(n.id, id);
            let mut props = Vec::with_capacity(count);
            let mut seen = HashSet::new();
            for i in 0..count {
                let at = 4 + i * 6;
                let pid = u16_at(&data, at)?;
                let form = u16_at(&data, at + 2)?;
                let len = u16_at(&data, at + 4)? as usize;
                if !seen.insert(pid) {
                    return Err(Error::invalid("duplicate property ID", at));
                }
                slice(&data, offset, len)?;
                props.push(Property {
                    id: pid,
                    stored_form: form,
                    bytes: offset..offset + len,
                    references: Vec::new(),
                });
                offset += len;
            }
            objects.push(Object {
                id,
                node: n.id,
                class: n.class,
                parent: None,
                properties: props,
                data,
            });
        }
        let root = *node_objects.get(&0).ok_or_else(|| Error::Missing {
            context: "AAF root",
            name: "properties".into(),
        })?;
        let mut diagnostics = Vec::new();
        let mut identifiers = HashMap::new();
        for obj in &mut objects {
            obj.parent = cfb.nodes[obj.node as usize]
                .parent
                .and_then(|id| node_objects.get(&id).copied());
            for p in &mut obj.properties {
                let b = &obj.data[p.bytes.clone()];
                if !matches!(p.stored_form, 0x22 | 0x32 | 0x3a) {
                    continue;
                }
                let name = utf16(b)?;
                if p.stored_form == 0x22 {
                    if let Some(id) = cfb
                        .child(obj.node, &name)
                        .and_then(|n| node_objects.get(&n).copied())
                    {
                        p.references.push(id);
                    } else {
                        diagnostics.push(Diagnostic {
                            object: Some(obj.id),
                            message: format!("missing strong reference {name}"),
                        });
                    }
                    continue;
                }
                let index = cfb
                    .child(obj.node, &format!("{name} index"))
                    .ok_or_else(|| Error::Missing {
                        context: "strong reference index",
                        name: name.clone(),
                    })?;
                let b = cfb.stream(index)?.to_vec();
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
                for i in 0..count {
                    let at = header + i * stride;
                    let key = u32_at(&b, at)?;
                    let target = format!("{name}{{{key:x}}}");
                    if let Some(id) = cfb
                        .child(obj.node, &target)
                        .and_then(|n| node_objects.get(&n).copied())
                    {
                        p.references.push(id);
                        if set {
                            identifiers.insert((pid, b[at + 8..at + 8 + keylen].to_vec()), id);
                        }
                    } else {
                        diagnostics.push(Diagnostic {
                            object: Some(obj.id),
                            message: format!("missing set/vector member {target}"),
                        });
                    }
                }
                if set {
                    p.references.reverse();
                }
            }
        }
        let mut doc = Self {
            cfb,
            path,
            objects,
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
    pub fn object(&self, id: ObjectId) -> &Object {
        &self.objects[id.0 as usize]
    }
    pub fn get_object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(id.0 as usize)
    }
    pub fn object_by_node(&self, id: u32) -> Option<&Object> {
        self.node_objects.get(&id).map(|id| self.object(*id))
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
