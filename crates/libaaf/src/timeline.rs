//! Interpreted audio timeline, existing single-clip video support, and software quirks.
#![allow(non_upper_case_globals)]
use crate::{Diagnostic, Document, Error, ObjectId, Result, Uid, bytes::*, defs::*};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rational {
    pub numerator: i32,
    pub denominator: i32,
}
impl Default for Rational {
    fn default() -> Self {
        Self {
            numerator: 1,
            denominator: 1,
        }
    }
}
impl Rational {
    pub const fn new(numerator: i32, denominator: i32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        Ok(Self {
            numerator: u32_at(b, 0)? as i32,
            denominator: u32_at(b, 4)? as i32,
        })
    }
    pub fn as_f64(self) -> f64 {
        if self.denominator == 0 {
            0.0
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
    /// Mirrors the upstream single-precision rate calculation and truncation.
    pub fn convert(self, value: i64, to: Self) -> i64 {
        if self == to {
            return value;
        }
        if self.denominator == 0 || self.numerator == 0 || to.denominator == 0 {
            return 0;
        }
        let a = (self.numerator as f32 / self.denominator as f32) as f64;
        let b = (to.numerator as f32 / to.denominator as f32) as f64;
        (value as f64 * (b / a)) as i64
    }
}
pub type Metadata = Vec<(String, String)>;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Interpolation {
    None,
    #[default]
    Linear,
    Log,
    Constant,
    Power,
    BSpline,
}
impl Interpolation {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "CURV_NON",
            Self::Linear => "CURV_LIN",
            Self::Log => "CURV_LOG",
            Self::Constant => "CURV_CST",
            Self::Power => "CURV_PWR",
            Self::BSpline => "CURV_BSP",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Curve {
    pub constant: bool,
    pub interpolation: Interpolation,
    pub points: Vec<(Rational, Rational)>,
}
impl Curve {
    pub fn value(&self) -> f64 {
        self.points.first().map(|(_, v)| v.as_f64()).unwrap_or(0.0)
    }
}
#[derive(Clone, Debug)]
pub struct Timecode {
    pub start: i64,
    pub fps: u16,
    pub drop: bool,
    pub edit_rate: Rational,
}
impl Default for Timecode {
    fn default() -> Self {
        Self {
            start: 0,
            fps: 25,
            drop: false,
            edit_rate: Rational::new(25, 1),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EssenceFormat {
    #[default]
    Unknown,
    Pcm,
    Wave,
    Aiff,
    Bwav,
    Video,
}
impl EssenceFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNK ",
            Self::Pcm => "PCM ",
            Self::Wave => "WAVE",
            Self::Aiff => "AIFC",
            Self::Bwav => "BWAV",
            Self::Video => "",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Essence {
    pub source_mob: ObjectId,
    pub source_slot: u32,
    pub master_mob: ObjectId,
    pub name: String,
    pub unique_name: String,
    pub format: EssenceFormat,
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_size: u16,
    pub length: i64,
    pub edit_rate: Rational,
    pub origin: i64,
    pub original_path: Option<String>,
    pub usable_path: Option<PathBuf>,
    pub embedded_node: Option<u32>,
    pub descriptor: ObjectId,
    pub data_offset: u64,
    pub metadata: Metadata,
    pub mob_id: [u8; 32],
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub essence: usize,
    pub channel: u32,
}
#[derive(Clone, Debug)]
pub struct Clip {
    pub object: ObjectId,
    pub position: i64,
    pub length: i64,
    pub source_offset: i64,
    pub channels: u16,
    pub sources: Vec<Source>,
    pub gain: Option<Curve>,
    pub automation: Option<Curve>,
    pub mute: bool,
    pub subclip_name: Option<String>,
    pub metadata: Metadata,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FadeKind {
    In,
    Out,
    Cross,
}
#[derive(Clone, Debug)]
pub struct Transition {
    pub length: i64,
    pub cut_point: i64,
    pub kind: FadeKind,
    pub interpolation: Interpolation,
    pub single_curve: bool,
}
impl Transition {
    /// The pinned reader uses the Edit Protocol's two endpoint fade, retaining
    /// the supplied interpolation. Custom control points remain in Document.
    pub fn level_curve(&self) -> Curve {
        let (start, end) = if self.kind == FadeKind::Out {
            (1, 0)
        } else {
            (0, 1)
        };
        Curve {
            constant: false,
            interpolation: self.interpolation,
            points: vec![
                (Rational::new(0, 1), Rational::new(start, 1)),
                (Rational::new(1, 1), Rational::new(end, 1)),
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Clip(Clip),
    Transition(Transition),
}
impl Item {
    pub fn clip(&self) -> Option<&Clip> {
        if let Self::Clip(c) = self {
            Some(c)
        } else {
            None
        }
    }
    pub fn clip_mut(&mut self) -> Option<&mut Clip> {
        if let Self::Clip(c) = self {
            Some(c)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug)]
pub struct Track {
    pub number: u32,
    pub name: Option<String>,
    pub edit_rate: Rational,
    pub channels: u16,
    pub video: bool,
    pub solo: bool,
    pub mute: bool,
    pub gain: Option<Curve>,
    pub pan: Option<Curve>,
    pub items: Vec<Item>,
    pub length: i64,
}
impl Track {
    fn new(number: u32, name: Option<String>, edit_rate: Rational, video: bool) -> Self {
        Self {
            number,
            name,
            edit_rate,
            video,
            channels: 0,
            solo: false,
            mute: false,
            gain: None,
            pan: None,
            items: Vec::new(),
            length: 0,
        }
    }
}
#[derive(Debug, Default)]
pub struct Composition {
    pub name: Option<String>,
    pub tracks: Vec<Track>,
    pub essences: Vec<Essence>,
    pub markers: Vec<Marker>,
    pub timecode: Timecode,
    pub length: i64,
    pub length_rate: Option<Rational>,
    pub sample_rate: u32,
    pub sample_size: u16,
    pub metadata: Metadata,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug)]
pub struct Marker {
    pub position: i64,
    pub length: i64,
    pub edit_rate: Rational,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub color: [u16; 3],
}
#[derive(Clone, Default)]
struct Context {
    gain: Option<Curve>,
    automation: Option<Curve>,
    muted: bool,
    warp: Option<Rational>,
    forced_length: Option<i64>,
}
struct Parser<'a> {
    doc: &'a Document,
    composition: Composition,
    essences: HashMap<(ObjectId, u32), usize>,
    active: HashSet<ObjectId>,
    resolve: bool,
}
fn add(a: i64, b: i64) -> Result<i64> {
    a.checked_add(b)
        .ok_or_else(|| Error::invalid("timeline arithmetic", 0))
}
fn metadata(doc: &Document, id: ObjectId) -> Metadata {
    let mut m = doc.tagged(id, PID_Mob_UserComments);
    m.reverse();
    m
}
pub fn parse(doc: &Document) -> Result<Composition> {
    let identification = doc.identification.map(|id| doc.object(id));
    let resolve = identification.is_some_and(|o| {
        o.string(PID_Identification_CompanyName)
            .is_some_and(|s| s.starts_with("Blackmagic Design"))
            && o.string(PID_Identification_ProductName)
                .is_some_and(|s| s.starts_with("DaVinci Resolve"))
    });
    let mut parser = Parser {
        doc,
        composition: Composition::default(),
        essences: HashMap::new(),
        active: HashSet::new(),
        resolve,
    };
    let edit =
        doc.object(doc.header).uid(PID_Header_OperationalPattern) == Some(AAFOPDef_EditProtocol);
    for &mob in doc.object(doc.content).references(PID_ContentStorage_Mobs) {
        let o = doc.object(mob);
        if o.class != AAFClassID_CompositionMob {
            continue;
        }
        let usage = o.uid(PID_Mob_UsageCode);
        if usage != Some(AAFUsage_TopLevel) && (edit || usage.is_some()) {
            continue;
        }
        if parser.composition.name.is_some() {
            parser.warn(mob, "multiple top-level compositions unsupported");
            break;
        }
        parser.composition.name = o.string(PID_Mob_Name);
        parser.composition.metadata = metadata(doc, mob);
        for &slot in o.references(PID_Mob_Slots) {
            let o = doc.object(slot);
            let Some(seg) = o.reference(PID_MobSlot_Segment) else {
                continue;
            };
            let rate = o
                .rational(PID_TimelineMobSlot_EditRate)
                .or_else(|| o.rational(PID_EventMobSlot_EditRate))
                .unwrap_or_default();
            let def = doc.weak_uid(seg, PID_Component_DataDefinition);
            let video = def == Some(AAFDataDef_Picture) || def == Some(AAFDataDef_LegacyPicture);
            let audio = def == Some(AAFDataDef_Sound) || def == Some(AAFDataDef_LegacySound);
            let number = o.u32(PID_MobSlot_PhysicalTrackNumber).unwrap_or(
                (parser
                    .composition
                    .tracks
                    .iter()
                    .filter(|t| t.video == video)
                    .count()
                    + 1) as u32,
            );
            let mut track = Track::new(number, o.string(PID_MobSlot_SlotName), rate, video);
            track.solo = doc
                .tagged_named(slot, "TimelineMobAttributeList", "AudioMixerCompSolo")
                .is_some_and(|v| v != "0");
            track.mute = doc
                .tagged_named(slot, "TimelineMobAttributeList", "AudioMixerCompMute")
                .is_some_and(|v| v != "0");
            parser.segment(seg, &mut track, Context::default(), true, None, None)?;
            if audio || video {
                if video && parser.composition.tracks.iter().any(|t| t.video) {
                    parser.warn(slot, "only one video track supported");
                    continue;
                }
                parser.composition.tracks.push(track);
            }
        }
    }
    parser.finish()?;
    Ok(parser.composition)
}
impl Parser<'_> {
    fn warn(&mut self, id: ObjectId, message: impl Into<String>) {
        self.composition.diagnostics.push(Diagnostic {
            object: Some(id),
            message: message.into(),
        });
    }
    fn enter(&mut self, id: ObjectId) -> Result<()> {
        if self.active.len() >= 256 {
            return Err(Error::Limit {
                context: "timeline recursion",
                limit: 256,
            });
        }
        if !self.active.insert(id) {
            return Err(Error::Cycle {
                context: "source references",
                id: id.0,
            });
        }
        Ok(())
    }
    fn segment(
        &mut self,
        id: ObjectId,
        track: &mut Track,
        mut ctx: Context,
        track_op: bool,
        prev: Option<ObjectId>,
        next: Option<ObjectId>,
    ) -> Result<()> {
        self.enter(id)?;
        let o = self.doc.object(id);
        let class = o.class;
        let len = o.i64(PID_Component_Length).unwrap_or(0);
        if len < 0 {
            return Err(Error::invalid("negative component length", 0));
        }
        if class == AAFClassID_Sequence {
            let num = self
                .doc
                .tagged_named(id, "ComponentAttributeList", "_MIXMATCH_RATE_NUM")
                .and_then(|s| s.parse().ok());
            let den = self
                .doc
                .tagged_named(id, "ComponentAttributeList", "_MIXMATCH_RATE_DENOM")
                .and_then(|s| s.parse().ok());
            if let (Some(n), Some(d)) = (num, den) {
                ctx.warp = Some(Rational::new(n, d));
            }
            let ids = o.references(PID_Sequence_Components);
            for (i, &child) in ids.iter().enumerate() {
                self.segment(
                    child,
                    track,
                    ctx.clone(),
                    false,
                    i.checked_sub(1).map(|i| ids[i]),
                    ids.get(i + 1).copied(),
                )?;
            }
        } else if class == AAFClassID_Filler {
            track.length = add(track.length, len)?;
        } else if class == AAFClassID_OperationGroup {
            let op = self.doc.weak_uid(id, PID_OperationGroup_Operation);
            self.parameters(id, op, track, &mut ctx, track_op);
            let inputs = o.references(PID_OperationGroup_InputSegments);
            if op == Some(AAFOperationDef_AudioChannelCombiner) {
                let start = track.length;
                let begin = track.items.len();
                let mut merged: Option<Clip> = None;
                for &input in inputs {
                    let mut channel_track = Track::new(
                        track.number,
                        track.name.clone(),
                        track.edit_rate,
                        track.video,
                    );
                    channel_track.length = start;
                    let mut ctx = ctx.clone();
                    if self.resolve && len > 0 {
                        ctx.forced_length = Some(len);
                    }
                    self.segment(input, &mut channel_track, ctx, false, None, None)?;
                    for item in channel_track.items {
                        if let Item::Clip(mut clip) = item {
                            if let Some(first) = &mut merged {
                                if first.sources.first().map(|s| s.essence)
                                    == clip.sources.first().map(|s| s.essence)
                                {
                                    if let Some(s) = first.sources.first_mut() {
                                        s.channel = 0;
                                    }
                                } else {
                                    first.sources.append(&mut clip.sources);
                                }
                            } else {
                                merged = Some(clip);
                            }
                        }
                    }
                }
                if let Some(c) = merged {
                    track.length = add(start, c.length)?;
                    track.items.truncate(begin);
                    track.items.push(Item::Clip(c));
                }
                track.channels = inputs.len() as u16;
            } else {
                for &input in inputs {
                    self.segment(input, track, ctx.clone(), track_op, None, None)?;
                }
            }
        } else if class == AAFClassID_Selector {
            let disabled =
                self.doc
                    .tagged_named(id, "ComponentAttributeList", "_DISABLE_CLIP_FLAG");
            let chosen = if disabled.is_some() {
                ctx.muted = disabled.as_deref() != Some("0");
                o.reference(PID_Selector_Alternates)
                    .or_else(|| o.reference(PID_Selector_Selected))
            } else {
                o.reference(PID_Selector_Selected)
            };
            if let Some(child) = chosen {
                self.segment(child, track, ctx, false, None, None)?;
            }
        } else if class == AAFClassID_NestedScope {
            for &child in o.references(PID_NestedScope_Slots) {
                self.segment(child, track, ctx.clone(), false, None, None)?;
            }
        } else if class == AAFClassID_SourceClip {
            if track.video && track.items.iter().any(|i| i.clip().is_some()) {
                self.warn(id, "only one video clip supported");
            } else {
                let rate = if track.video {
                    track.edit_rate
                } else {
                    ctx.warp.unwrap_or(track.edit_rate)
                };
                let length = rate.convert(ctx.forced_length.unwrap_or(len), track.edit_rate);
                let offset = rate.convert(
                    o.i64(PID_SourceClip_StartTime).unwrap_or(0),
                    track.edit_rate,
                );
                let mut clip = Clip {
                    object: id,
                    position: track.length,
                    length,
                    source_offset: offset,
                    channels: 0,
                    sources: Vec::new(),
                    gain: ctx.gain.clone(),
                    automation: ctx.automation.clone(),
                    mute: ctx.muted,
                    subclip_name: None,
                    metadata: Vec::new(),
                };
                if let Some(comment) =
                    self.doc
                        .tagged_named(id, "ComponentAttributeList", "_COMMENT")
                {
                    clip.metadata.push(("_COMMENT".into(), comment));
                }
                self.resolve_clip(id, &mut clip, track.video)?;
                track.length = add(track.length, length)?;
                if !track.video && track.channels == 0 {
                    track.channels = 1;
                }
                track.items.push(Item::Clip(clip));
            }
        } else if class == AAFClassID_Transition {
            let kind = if prev.is_some_and(|id| self.doc.object(id).class == AAFClassID_Filler) {
                FadeKind::In
            } else if next.is_some_and(|id| self.doc.object(id).class == AAFClassID_Filler) {
                FadeKind::Out
            } else {
                FadeKind::Cross
            };
            let op = o.reference(PID_Transition_OperationGroup);
            let mut interpolation = Interpolation::Linear;
            if let Some(op) = op {
                for &pid in self
                    .doc
                    .object(op)
                    .references(PID_OperationGroup_Parameters)
                {
                    if let Some(c) = self.curve(pid) {
                        interpolation = c.interpolation;
                    }
                }
                if let Some(curve) = self.doc.tagged_named(
                    op,
                    "ComponentAttributeList",
                    "_ATN_AUDIO_DISSOLVE_CURVETYPE",
                ) {
                    interpolation = if curve == "1" {
                        Interpolation::Power
                    } else {
                        Interpolation::Linear
                    };
                }
            }
            track.items.push(Item::Transition(Transition {
                length: len,
                cut_point: o.i64(PID_Transition_CutPoint).unwrap_or(len / 2),
                kind,
                interpolation,
                single_curve: true,
            }));
            track.length = add(track.length, -len)?;
        } else if class == AAFClassID_Timecode {
            if self.composition.timecode == Timecode::default() {
                self.composition.timecode = Timecode {
                    start: o.i64(PID_Timecode_Start).unwrap_or(0),
                    fps: o.u16(PID_Timecode_FPS).unwrap_or(25),
                    drop: o
                        .raw(PID_Timecode_Drop)
                        .is_some_and(|b| b.first() == Some(&1)),
                    edit_rate: track.edit_rate,
                };
            }
        } else if class == AAFClassID_DescriptiveMarker {
            let color = self
                .doc
                .named_raw(id, "CommentMarkerColor")
                .filter(|b| b.len() == 6)
                .map(|b| {
                    [
                        u16::from_be_bytes([b[0], b[1]]),
                        u16::from_be_bytes([b[2], b[3]]),
                        u16::from_be_bytes([b[4], b[5]]),
                    ]
                })
                .unwrap_or_default();
            let name = self
                .doc
                .named_raw(id, "CommentMarkerUser")
                .or_else(|| self.doc.named_raw(id, "CommentMarkerUSer"))
                .and_then(|b| utf16(b).ok());
            self.composition.markers.push(Marker {
                position: o.i64(PID_Event_Position).unwrap_or(0),
                length: len,
                edit_rate: track.edit_rate,
                name,
                comment: o.string(PID_Event_Comment),
                color,
            });
        } else {
            self.warn(
                id,
                format!("unsupported segment {}", self.doc.class_name(class)),
            );
        }
        self.active.remove(&id);
        Ok(())
    }
    fn curve(&self, id: ObjectId) -> Option<Curve> {
        let o = self.doc.object(id);
        if o.class == AAFClassID_ConstantValue {
            let (typ, b) = o.indirect(PID_ConstantValue_Value)?;
            if typ != AAFTypeID_Rational {
                return None;
            }
            return Some(Curve {
                constant: true,
                interpolation: Interpolation::None,
                points: vec![(Rational::new(0, 1), Rational::from_bytes(b).ok()?)],
            });
        }
        if o.class != AAFClassID_VaryingValue {
            return None;
        }
        let interp = self.doc.weak_uid(id, PID_VaryingValue_Interpolation);
        let interpolation = match interp {
            Some(AAFInterpolationDef_None) => Interpolation::None,
            Some(AAFInterpolationDef_Log) => Interpolation::Log,
            Some(AAFInterpolationDef_Constant) => Interpolation::Constant,
            Some(AAFInterpolationDef_Power) => Interpolation::Power,
            Some(AAFInterpolationDef_BSpline) => Interpolation::BSpline,
            _ => Interpolation::Linear,
        };
        let points: Vec<_> = o
            .references(PID_VaryingValue_PointList)
            .iter()
            .filter_map(|&id| {
                let o = self.doc.object(id);
                let time = o.rational(PID_ControlPoint_Time)?;
                let (typ, b) = o.indirect(PID_ControlPoint_Value)?;
                if typ != AAFTypeID_Rational {
                    return None;
                }
                Some((time, Rational::from_bytes(b).ok()?))
            })
            .collect();
        if points.is_empty() {
            return None;
        }
        let constant = points.len() == 2 && points[0].1 == points[1].1;
        Some(Curve {
            constant,
            interpolation,
            points,
        })
    }
    fn parameters(
        &mut self,
        id: ObjectId,
        op: Option<Uid>,
        track: &mut Track,
        ctx: &mut Context,
        affect_track: bool,
    ) {
        for &pid in self
            .doc
            .object(id)
            .references(PID_OperationGroup_Parameters)
        {
            let o = self.doc.object(pid);
            let def = o.uid(PID_Parameter_Definition);
            let Some(curve) = self.curve(pid) else {
                continue;
            };
            if op == Some(AAFOperationDef_MonoAudioGain) {
                if def != Some(AAFParameterDef_Amplitude)
                    && !self.doc.objects().any(|o| {
                        o.uid(PID_DefinitionObject_Identification) == def
                            && o.string(PID_DefinitionObject_Name).as_deref()
                                == Some("AvidControlClipRatio")
                    })
                {
                    continue;
                }
                if o.class == AAFClassID_VaryingValue && curve.constant && curve.value() == 1.0 {
                    continue;
                }
                if affect_track {
                    if track.gain.is_none() {
                        track.gain = Some(curve);
                    }
                } else if curve.constant {
                    if ctx.gain.is_none() {
                        ctx.gain = Some(curve);
                    }
                } else if ctx.automation.is_none() {
                    ctx.automation = Some(curve);
                }
            } else if op == Some(AAFOperationDef_MonoAudioPan)
                && def == Some(AAFParameterDef_Pan)
                && affect_track
            {
                track.pan = Some(curve);
            }
        }
    }
    fn target(&self, id: ObjectId) -> Option<(ObjectId, ObjectId)> {
        let o = self.doc.object(id);
        let mob = self.doc.mob(o.raw(PID_SourceReference_SourceID)?)?;
        let slot = o.u32(PID_SourceReference_SourceMobSlotID)?;
        let slot = self
            .doc
            .object(mob)
            .references(PID_Mob_Slots)
            .iter()
            .copied()
            .find(|&id| self.doc.object(id).u32(PID_MobSlot_SlotID) == Some(slot))?;
        Some((mob, slot))
    }
    fn resolve_clip(&mut self, id: ObjectId, clip: &mut Clip, video: bool) -> Result<()> {
        let Some((mob, slot)) = self.target(id) else {
            self.warn(id, "missing source mob or slot");
            return Ok(());
        };
        self.enter(mob)?;
        let o = self.doc.object(mob);
        if o.class == AAFClassID_MasterMob {
            if let Some(seg) = self.doc.object(slot).reference(PID_MobSlot_Segment) {
                self.resolve_master(seg, mob, slot, clip, video)?;
            }
        } else if o.class == AAFClassID_CompositionMob {
            if o.uid(PID_Mob_UsageCode) == Some(AAFUsage_SubClip) {
                clip.subclip_name = o.string(PID_Mob_Name);
                let mut m = metadata(self.doc, mob);
                m.append(&mut clip.metadata);
                clip.metadata = m;
            }
            if let Some(seg) = self.doc.object(slot).reference(PID_MobSlot_Segment) {
                self.derivation(
                    seg,
                    clip,
                    video,
                    o.uid(PID_Mob_UsageCode) == Some(AAFUsage_AdjustedClip),
                )?;
            }
        } else {
            self.warn(
                id,
                "source clip does not target MasterMob or CompositionMob",
            );
        }
        self.active.remove(&mob);
        Ok(())
    }
    fn derivation(
        &mut self,
        id: ObjectId,
        clip: &mut Clip,
        video: bool,
        adjusted: bool,
    ) -> Result<()> {
        self.enter(id)?;
        let o = self.doc.object(id);
        if o.class == AAFClassID_SourceClip {
            self.resolve_clip(id, clip, video)?;
        } else if o.class == AAFClassID_OperationGroup {
            if !adjusted
                && self.doc.weak_uid(id, PID_OperationGroup_Operation)
                    == Some(AAFOperationDef_MonoAudioGain)
            {
                for &param in o.references(PID_OperationGroup_Parameters) {
                    if let Some(c) = self.curve(param) {
                        if c.constant && clip.gain.is_none() {
                            clip.gain = Some(c);
                        } else if !c.constant && clip.automation.is_none() {
                            clip.automation = Some(c);
                        }
                    }
                }
            }
            for &child in o.references(PID_OperationGroup_InputSegments) {
                self.derivation(child, clip, video, adjusted)?;
            }
        } else if o.class == AAFClassID_Sequence {
            for &child in o.references(PID_Sequence_Components) {
                self.derivation(child, clip, video, adjusted)?;
            }
        }
        self.active.remove(&id);
        Ok(())
    }
    fn resolve_master(
        &mut self,
        id: ObjectId,
        master: ObjectId,
        master_slot: ObjectId,
        clip: &mut Clip,
        video: bool,
    ) -> Result<()> {
        self.enter(id)?;
        let o = self.doc.object(id);
        if o.class == AAFClassID_Sequence {
            for &child in o.references(PID_Sequence_Components) {
                self.resolve_master(child, master, master_slot, clip, video)?;
            }
        } else if let Some((source, slot)) = self.target(id) {
            let key = (
                source,
                self.doc.object(slot).u32(PID_MobSlot_SlotID).unwrap_or(0),
            );
            let essence = if let Some(&e) = self.essences.get(&key) {
                e
            } else {
                let e = self.essence(master, source, slot, video)?;
                let index = self.composition.essences.len();
                self.composition.essences.push(e);
                self.essences.insert(key, index);
                index
            };
            clip.sources.push(Source {
                essence,
                channel: self
                    .doc
                    .object(master_slot)
                    .u32(PID_MobSlot_PhysicalTrackNumber)
                    .unwrap_or(0),
            });
        } else {
            self.warn(id, "missing master source reference");
        }
        self.active.remove(&id);
        Ok(())
    }
    fn essence(
        &mut self,
        master: ObjectId,
        source: ObjectId,
        slot: ObjectId,
        video: bool,
    ) -> Result<Essence> {
        let doc = self.doc;
        let src = doc.object(source);
        let descriptor = src
            .reference(PID_SourceMob_EssenceDescription)
            .ok_or_else(|| Error::Missing {
                context: "essence descriptor",
                name: source.0.to_string(),
            })?;
        let desc = doc.object(descriptor);
        let class = desc.class;
        let format = if video {
            EssenceFormat::Video
        } else if class == AAFClassID_PCMDescriptor {
            EssenceFormat::Pcm
        } else if class == AAFClassID_WAVEDescriptor {
            EssenceFormat::Wave
        } else if class == AAFClassID_AIFCDescriptor {
            EssenceFormat::Aiff
        } else {
            EssenceFormat::Unknown
        };
        let name = doc
            .object(master)
            .string(PID_Mob_Name)
            .unwrap_or_else(|| "unknown".into());
        let mob_id: [u8; 32] = src
            .raw(PID_Mob_MobID)
            .and_then(|b| b.try_into().ok())
            .unwrap_or_default();
        let mut unique_name = if doc.options.mobid_filenames {
            Uid::from_bytes(&mob_id[16..])
                .unwrap()
                .to_string()
                .replace("-", "")
        } else {
            name.clone()
        };
        if doc.options.mobid_filenames {
            let u = Uid::from_bytes(&mob_id[16..]).unwrap().to_string();
            unique_name = format!("{}{}", &u[..23], u[23..].replace('-', ""));
        }
        let mut index = 0;
        while self
            .composition
            .essences
            .iter()
            .any(|e| (e.format == EssenceFormat::Video) == video && e.unique_name == unique_name)
        {
            index += 1;
            unique_name = format!("{name}_{index}");
        }
        let original_path = desc
            .references(PID_EssenceDescriptor_Locator)
            .iter()
            .find_map(|&id| doc.object(id).string(PID_NetworkLocator_URLString));
        let embedded_node = doc
            .object(doc.content)
            .references(PID_ContentStorage_EssenceData)
            .iter()
            .copied()
            .find(|&id| doc.object(id).raw(PID_EssenceData_MobID) == Some(&mob_id[..]))
            .and_then(|id| {
                let o = doc.object(id);
                let name = utf16(o.raw(PID_EssenceData_Data)?.get(1..)?).ok()?;
                doc.cfb.child(o.node, &name)
            });
        let mut e = Essence {
            source_mob: source,
            source_slot: doc.object(slot).u32(PID_MobSlot_SlotID).unwrap_or(0),
            master_mob: master,
            name,
            unique_name,
            format,
            channels: if format == EssenceFormat::Unknown {
                0
            } else {
                desc.u32(PID_SoundDescriptor_Channels).unwrap_or(0) as u16
            },
            sample_rate: 0,
            sample_size: if format == EssenceFormat::Unknown {
                0
            } else {
                desc.u32(PID_SoundDescriptor_QuantizationBits).unwrap_or(0) as u16
            },
            length: if video || format == EssenceFormat::Unknown {
                0
            } else {
                desc.i64(PID_FileDescriptor_Length).unwrap_or(0)
            },
            edit_rate: desc
                .rational(PID_FileDescriptor_SampleRate)
                .unwrap_or_default(),
            origin: doc
                .object(slot)
                .i64(PID_TimelineMobSlot_Origin)
                .unwrap_or(0),
            original_path,
            usable_path: None,
            embedded_node,
            descriptor,
            data_offset: 0,
            metadata: metadata(doc, master),
            mob_id,
        };
        if format == EssenceFormat::Pcm {
            e.sample_rate = e.edit_rate.as_f64() as u32;
        }
        if let Some(b) = desc
            .raw(PID_WAVEDescriptor_Summary)
            .or_else(|| desc.raw(PID_AIFCDescriptor_Summary))
            && let Ok(info) = crate::media::audio_info(b)
        {
            e.channels = info.channels;
            e.sample_rate = info.sample_rate;
            e.sample_size = info.sample_size;
            e.length = info.frames as i64;
            e.data_offset = info.data_offset;
            e.edit_rate = Rational::new(info.sample_rate as i32, 1);
        }
        if matches!(format, EssenceFormat::Wave | EssenceFormat::Aiff)
            && let Some(node) = embedded_node
        {
            let stream = doc.cfb.stream(node)?;
            if let Ok(info) = crate::media::audio_info(&stream.prefix(65536)?) {
                e.channels = info.channels;
                e.sample_rate = info.sample_rate;
                e.sample_size = info.sample_size;
                e.length = info.frames as i64;
                e.data_offset = info.data_offset;
                e.edit_rate = Rational::new(info.sample_rate as i32, 1);
            }
        }
        Ok(e)
    }
    fn finish(&mut self) -> Result<()> {
        crate::media::resolve_all(self.doc, &mut self.composition)?;
        // Upstream prepends essences. Stable source IDs are translated once.
        let n = self.composition.essences.len();
        self.composition.essences.reverse();
        for track in &mut self.composition.tracks {
            for item in &mut track.items {
                if let Item::Clip(c) = item {
                    for s in &mut c.sources {
                        s.essence = n - 1 - s.essence;
                    }
                    c.channels = c
                        .sources
                        .iter()
                        .map(|s| {
                            if s.channel == 0 {
                                self.composition.essences[s.essence].channels
                            } else {
                                1
                            }
                        })
                        .try_fold(0u16, |total, channels| total.checked_add(channels))
                        .ok_or_else(|| Error::invalid("combined channel count", 0))?;
                }
            }
        }
        for track in self
            .composition
            .tracks
            .iter()
            .filter(|t| !t.video)
            .chain(self.composition.tracks.iter().filter(|t| t.video))
        {
            let end = self
                .composition
                .length_rate
                .map(|rate| track.edit_rate.convert(track.length, rate))
                .unwrap_or(track.length);
            if end > self.composition.length {
                self.composition.length = track.length;
                self.composition.length_rate = Some(track.edit_rate);
            }
        }
        let mut counts = HashMap::new();
        for e in &self.composition.essences {
            if e.format != EssenceFormat::Video {
                *counts
                    .entry((e.sample_rate, e.sample_size))
                    .or_insert(0usize) += 1;
            }
        }
        let mut max = 0;
        for e in &self.composition.essences {
            let count = counts
                .get(&(e.sample_rate, e.sample_size))
                .copied()
                .unwrap_or(0);
            if count > max {
                max = count;
                self.composition.sample_rate = e.sample_rate;
                self.composition.sample_size = e.sample_size;
            }
        }
        if let Some(id) = self.doc.identification {
            let o = self.doc.object(id);
            if o.string(PID_Identification_CompanyName).as_deref() == Some("Digidesign, Inc.")
                && o.string(PID_Identification_ProductName).as_deref() == Some("ProTools")
            {
                post_protools(
                    &mut self.composition,
                    self.doc.options.remove_sample_accurate_edits,
                    self.doc.options.recover_protools_fades,
                )?;
            }
        }
        // Validate absolute display positions as well as relative edit positions.
        let c = &self.composition;
        for t in &c.tracks {
            let start = c.timecode.edit_rate.convert(c.timecode.start, t.edit_rate);
            for clip in t.items.iter().filter_map(Item::clip) {
                add(add(clip.position, start)?, clip.length)?;
            }
        }
        for marker in &c.markers {
            add(
                marker.position,
                c.timecode
                    .edit_rate
                    .convert(c.timecode.start, marker.edit_rate),
            )?;
        }
        Ok(())
    }
}
impl PartialEq for Timecode {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
            && self.fps == other.fps
            && self.drop == other.drop
            && self.edit_rate == other.edit_rate
    }
}
fn is_sae(name: &str) -> bool {
    [
        "Sample accurate edit",
        "Samplegenaue Bearbeitung",
        "Edición con precisión de muestra",
        "Modification à l'échantillon près",
        "サンプル精度編集",
        "샘플 단위 정밀 편집",
        "精确采样编辑",
        "精確取樣編輯",
    ]
    .contains(&name)
}
fn is_fade(name: &str) -> bool {
    ["Fade ", "Fondu ", "Fundido ", "페이드", "淡变 ", "淡變 "].contains(&name)
}
fn clip_name<'a>(c: &Clip, essences: &'a [Essence]) -> &'a str {
    c.sources
        .first()
        .map(|s| essences[s.essence].name.as_str())
        .unwrap_or("")
}
fn remove_sae(track: &mut Track, i: usize, essences: &[Essence]) -> bool {
    let Some(c) = track.items[i].clip().cloned() else {
        return false;
    };
    if i > 0
        && let Some(left) = track.items[i - 1].clip_mut()
        && left.position.checked_add(left.length) == Some(c.position)
        && let Some(s) = left.sources.first()
    {
        let e = &essences[s.essence];
        let length = e.edit_rate.convert(e.length, track.edit_rate);
        if length
            .checked_sub(left.source_offset)
            .and_then(|v| v.checked_sub(left.length))
            .is_some_and(|v| v >= c.length)
            && let Some(length) = left.length.checked_add(c.length)
        {
            left.length = length;
            track.items.remove(i);
            return true;
        }
    }
    if let Some(right) = track.items.get_mut(i + 1).and_then(Item::clip_mut)
        && c.position.checked_add(c.length) == Some(right.position)
        && right.source_offset >= c.length
        && let (Some(position), Some(length), Some(offset)) = (
            right.position.checked_sub(c.length),
            right.length.checked_add(c.length),
            right.source_offset.checked_sub(c.length),
        )
    {
        right.position = position;
        right.length = length;
        right.source_offset = offset;
        track.items.remove(i);
        return true;
    }
    false
}
fn post_protools(c: &mut Composition, sae: bool, fades: bool) -> Result<()> {
    for track in &mut c.tracks {
        let mut i = 0;
        while i < track.items.len() {
            let Some(clip) = track.items[i].clip().cloned() else {
                i += 1;
                continue;
            };
            let name = clip_name(&clip, &c.essences);
            if sae && is_sae(name) && remove_sae(track, i, &c.essences) {
                i = 0;
                continue;
            }
            if fades && is_fade(name) {
                if i > 0
                    && track.items[i - 1]
                        .clip()
                        .is_some_and(|c1| is_sae(clip_name(c1, &c.essences)))
                    && remove_sae(track, i - 1, &c.essences)
                {
                    i = 0;
                    continue;
                }
                if track
                    .items
                    .get(i + 1)
                    .and_then(Item::clip)
                    .is_some_and(|c1| is_sae(clip_name(c1, &c.essences)))
                    && remove_sae(track, i + 1, &c.essences)
                {
                    i = 0;
                    continue;
                }
                let left = i
                    .checked_sub(1)
                    .and_then(|i| track.items[i].clip())
                    .filter(|p| p.position.checked_add(p.length) == Some(clip.position));
                let right = track
                    .items
                    .get(i + 1)
                    .and_then(Item::clip)
                    .filter(|p| clip.position.checked_add(clip.length) == Some(p.position));
                let has_left = left.is_some();
                let has_right = right.is_some();
                let enough_left = left.is_none_or(|p| {
                    p.sources.first().is_some_and(|s| {
                        let e = &c.essences[s.essence];
                        e.edit_rate
                            .convert(e.length, track.edit_rate)
                            .checked_sub(p.source_offset)
                            .and_then(|v| v.checked_sub(p.length))
                            .is_some_and(|v| v >= clip.length)
                    })
                });
                let enough_right = right.is_none_or(|p| p.source_offset >= clip.length);
                if (has_left || has_right) && enough_left && enough_right {
                    if has_left {
                        let p = track.items[i - 1].clip_mut().unwrap();
                        p.length = add(p.length, clip.length)?;
                    }
                    if has_right {
                        let p = track.items[i + 1].clip_mut().unwrap();
                        p.position = add(p.position, -clip.length)?;
                        p.length = add(p.length, clip.length)?;
                        p.source_offset = add(p.source_offset, -clip.length)?;
                    }
                    let kind = if has_left && has_right {
                        FadeKind::Cross
                    } else if has_left {
                        FadeKind::Out
                    } else {
                        FadeKind::In
                    };
                    track.items[i] = Item::Transition(Transition {
                        length: clip.length,
                        cut_point: match kind {
                            FadeKind::Cross => clip.length / 2,
                            FadeKind::Out => clip.length,
                            FadeKind::In => 0,
                        },
                        kind,
                        interpolation: Interpolation::Linear,
                        single_curve: false,
                    });
                    i = 0;
                    continue;
                }
            }
            i += 1;
        }
    }
    Ok(())
}
