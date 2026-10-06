//! Text inspection compatible with aaftool's analysis output.
use crate::{Document, ObjectId, Uid, bytes::*, defs::*, timeline::*};
use std::{fmt::Write, path::Path};
#[derive(Clone, Copy, Debug, Default)]
pub enum PositionFormat {
    #[default]
    Timecode,
    Samples,
    Hms,
    Raw,
}
#[derive(Clone, Debug, Default)]
pub struct DisplayOptions {
    pub position: PositionFormat,
    pub sample_rate: Option<u32>,
    pub automation: bool,
    pub metadata: bool,
    pub relative_paths: bool,
}
fn text(v: Option<String>) -> String {
    v.unwrap_or_else(|| "n/a".into())
}
pub fn uid_text(uid: Uid) -> String {
    let b = uid.0;
    format!(
        "{{ 0x{:08x} 0x{:04x} 0x{:04x} {{ {} }} }}",
        u32::from_le_bytes(b[..4].try_into().unwrap()),
        u16::from_le_bytes(b[4..6].try_into().unwrap()),
        u16::from_le_bytes(b[6..8].try_into().unwrap()),
        b[8..]
            .iter()
            .map(|b| format!("0x{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
fn timestamp(b: Option<&[u8]>) -> String {
    let Some(b) = b.filter(|b| b.len() == 8) else {
        return "n/a".into();
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:02}",
        u16_at(b, 0).unwrap() as i16,
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7]
    )
}
fn version(b: Option<&[u8]>) -> String {
    let Some(b) = b.filter(|b| b.len() >= 9) else {
        return "n/a".into();
    };
    let kind = [
        "AAFVersionUnknown",
        "AAFVersionReleased",
        "AAFVersionDebug",
        "AAFVersionPatched",
        "AAFVersionBeta",
        "AAFVersionPrivateBuild",
    ]
    .get(b[8] as usize)
    .copied()
    .unwrap_or("unknown");
    format!(
        "{}.{}.{}.{} {} ({})",
        u16_at(b, 0).unwrap(),
        u16_at(b, 2).unwrap(),
        u16_at(b, 4).unwrap(),
        u16_at(b, 6).unwrap(),
        kind,
        b[8]
    )
}
pub fn timecode(value: i64, rate: Rational, tc: &Timecode) -> String {
    let tc_rate = tc.edit_rate.numerator as f32 / tc.edit_rate.denominator as f32;
    let fps = [
        24000.0f32 / 1001.0,
        24.0,
        25.0,
        30000.0 / 1001.0,
        30.0,
        48000.0 / 1001.0,
        48.0,
        50.0,
        60000.0 / 1001.0,
        60.0,
        72.0,
        96.0,
        100.0,
        119.88,
        120.0,
    ]
    .into_iter()
    .find(|v| (*v * 100.0) as u32 == (tc_rate * 100.0) as u32)
    .unwrap_or(tc.fps.max(1) as f32);
    let unit = rate.numerator as f32 / rate.denominator as f32;
    let frames = if fps == unit {
        value
    } else {
        (value as f32 / (unit / fps)) as i64
    };
    let mut n = frames.unsigned_abs() as i128;
    let nominal = fps.round().max(1.0) as i128;
    let drop = tc.drop && matches!(nominal, 30 | 60 | 120);
    if drop {
        let per_minute = nominal * 60 - 2;
        let per_ten = per_minute * 10 + 2;
        let tens = n / per_ten;
        let mins = ((n % per_ten - 2) / per_minute).max(0);
        let multiplier = if nominal == 60 {
            2
        } else if nominal == 120 {
            4
        } else {
            1
        };
        n += (18 * tens + 2 * mins) * multiplier;
    }
    format!(
        "{}{:02}:{:02}:{:02}{}{:02}",
        if frames < 0 { "-" } else { "" },
        n / nominal / 3600,
        n / nominal / 60 % 60,
        n / nominal % 60,
        if drop { ';' } else { ':' },
        n % nominal
    )
}
fn pos(value: i64, rate: Rational, c: &Composition, o: &DisplayOptions, sr: Option<u32>) -> String {
    match o.position {
        PositionFormat::Raw => value.to_string(),
        PositionFormat::Samples => rate
            .convert(
                value,
                Rational::new(sr.or(o.sample_rate).unwrap_or(c.sample_rate) as i32, 1),
            )
            .to_string(),
        PositionFormat::Timecode => timecode(value, rate, &c.timecode),
        PositionFormat::Hms => {
            let r = rate.as_f64() as i64;
            let h = if r > 0 { value / (r * 3600) } else { 0 };
            let m = if r > 0 {
                value % (r * 3600) / (r * 60)
            } else {
                0
            };
            let s = if r > 0 { value % (r * 60) / r } else { 0 };
            let ms = if r >= 1000 { value % r / (r / 1000) } else { 0 };
            format!("{h:02}:{m:02}:{s:02}.{ms:03}")
        }
    }
}
fn meta(out: &mut String, m: &Metadata, prefix: &str, extra_space: bool) {
    let longest = m.iter().map(|(n, _)| n.len()).max().unwrap_or(0) + 1;
    for (n, v) in m {
        writeln!(
            out,
            "{prefix}- Name: \"{n}\"{}{} Text: \"{v}\"",
            if extra_space { " " } else { "" },
            " ".repeat(longest - n.len())
        )
        .unwrap();
    }
}
pub fn summary(doc: &Document, c: &Composition, o: &DisplayOptions) -> String {
    let mut out = String::new();
    let h = doc.object(doc.header);
    let b = h.u16(PID_Header_ByteOrder).unwrap_or(0);
    writeln!(
        out,
        " ByteOrder            : {} (0x{b:04x})",
        if b == 0x4949 {
            "Little-Endian"
        } else {
            "Big-Endian"
        }
    )
    .unwrap();
    writeln!(
        out,
        " LastModified         : {}",
        timestamp(h.raw(PID_Header_LastModified))
    )
    .unwrap();
    writeln!(
        out,
        " AAF ObjSpec Version  : {}",
        h.raw(PID_Header_Version)
            .filter(|b| b.len() == 2)
            .map(|b| format!("{}.{}", b[0], b[1]))
            .unwrap_or_else(|| "n/a".into())
    )
    .unwrap();
    writeln!(
        out,
        " ObjectModel Version  : {}",
        h.u32(PID_Header_ObjectModelVersion).unwrap_or(0)
    )
    .unwrap();
    writeln!(
        out,
        " Operational Pattern  : {}\n\n\n",
        h.uid(PID_Header_OperationalPattern)
            .and_then(|u| u.name().map(str::to_owned))
            .unwrap_or_else(|| "AAFUID_NULL".into())
    )
    .unwrap();
    if let Some(id) = doc.identification {
        let i = doc.object(id);
        for (label, pid) in [
            ("CompanyName", PID_Identification_CompanyName),
            ("ProductName", PID_Identification_ProductName),
        ] {
            writeln!(out, " {label:<21}: {}", text(i.string(pid))).unwrap();
        }
        writeln!(
            out,
            " ProductVersion       : {}",
            version(i.raw(PID_Identification_ProductVersion))
        )
        .unwrap();
        writeln!(
            out,
            " ProductVersionString : {}",
            text(i.string(PID_Identification_ProductVersionString))
        )
        .unwrap();
        writeln!(
            out,
            " ProductID            : {}",
            uid_text(i.uid(PID_Identification_ProductID).unwrap_or_default())
        )
        .unwrap();
        writeln!(
            out,
            " Date                 : {}",
            timestamp(i.raw(PID_Identification_Date))
        )
        .unwrap();
        writeln!(
            out,
            " ToolkitVersion       : {}",
            version(i.raw(PID_Identification_ToolkitVersion))
        )
        .unwrap();
        writeln!(
            out,
            " Platform             : {}",
            text(i.string(PID_Identification_Platform))
        )
        .unwrap();
        writeln!(
            out,
            " GenerationAUID       : {}\n\n\n",
            uid_text(i.uid(PID_Identification_GenerationAUID).unwrap_or_default())
        )
        .unwrap();
    }
    let tc = &c.timecode;
    let rate = c.length_rate.unwrap_or(Rational::new(0, 1));
    let start = if c.length_rate.is_some() {
        tc.edit_rate.convert(tc.start, rate)
    } else {
        tc.start
    };
    let end = start.saturating_add(c.length);
    let sr = Rational::new(o.sample_rate.unwrap_or(c.sample_rate) as i32, 1);
    writeln!(
        out,
        " Composition Name            : {}\n",
        c.name.as_deref().unwrap_or("(null)")
    )
    .unwrap();
    writeln!(
        out,
        " TC EditRrate                : {}/{}",
        tc.edit_rate.numerator, tc.edit_rate.denominator
    )
    .unwrap();
    writeln!(
        out,
        " TC FPS                      : {} {}\n",
        tc.fps,
        if tc.drop { "DF" } else { "NDF" }
    )
    .unwrap();
    writeln!(out, " Composition Start (EU)      : {}", tc.start).unwrap();
    writeln!(
        out,
        " Composition Start (samples) : {}",
        tc.edit_rate.convert(tc.start, sr)
    )
    .unwrap();
    writeln!(
        out,
        " Composition Start           : {}\n",
        timecode(tc.start, tc.edit_rate, tc)
    )
    .unwrap();
    writeln!(
        out,
        " Composition End (EU)        : {} (EditRate: {}/{})",
        end,
        if c.length_rate.is_some() {
            rate.numerator
        } else {
            0
        },
        if c.length_rate.is_some() {
            rate.denominator
        } else {
            0
        }
    )
    .unwrap();
    writeln!(
        out,
        " Composition End (samples)   : {}",
        if c.length_rate.is_some() {
            rate.convert(end, sr)
        } else {
            end
        }
    )
    .unwrap();
    writeln!(
        out,
        " Composition End             : {}\n",
        timecode(
            if c.length_rate.is_some() {
                rate.convert(end, tc.edit_rate)
            } else {
                end
            },
            tc.edit_rate,
            tc
        )
    )
    .unwrap();
    writeln!(out, " Dominant Sample Rate        : {}", c.sample_rate).unwrap();
    writeln!(out, " Dominant Sample Size        : {} bits", c.sample_size).unwrap();
    if !c.metadata.is_empty() {
        out.push_str("\n Metadata:\n");
        meta(&mut out, &c.metadata, " ", false);
    }
    out.push_str("\n\n");
    out
}
fn relative(path: &Path, base: &Path) -> String {
    let a: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let mut p = std::path::PathBuf::new();
    for _ in common..b.len() {
        p.push("..");
    }
    for v in &a[common..] {
        p.push(v.as_os_str());
    }
    if common == b.len() {
        format!("./{}", p.to_string_lossy())
    } else {
        p.to_string_lossy().into_owned()
    }
}
pub fn essences(doc: &Document, c: &Composition, o: &DisplayOptions) -> String {
    let mut out = String::from("Media Essences :\n================\n\n");
    for video in [true, false] {
        for (index, e) in c
            .essences
            .iter()
            .filter(|e| (e.format == EssenceFormat::Video) == video)
            .enumerate()
        {
            let index = index + 1;
            if video {
                writeln!(
                    out,
                    " Video[{index}] :: Length: {}  Name: \"{}\"  UniqueName: \"{}\"",
                    pos(
                        e.length,
                        Rational::new(o.sample_rate.unwrap_or(c.sample_rate) as i32, 1),
                        c,
                        o,
                        None
                    ),
                    e.name,
                    e.unique_name
                )
                .unwrap();
            } else {
                writeln!(out," Audio[{index}] :: Format: {} - {:02} ch - {} Hz - {} bits  Length: {}  Name: \"{}\"  UniqueName: \"{}\"",if e.format==EssenceFormat::Unknown{""}else{e.format.label()},e.channels,format_args!("{:5}",e.sample_rate),format_args!("{:2}",e.sample_size),pos(e.length,e.edit_rate,c,o,Some(e.sample_rate)),e.name,e.unique_name).unwrap();
            }
            let path = if e.embedded_node.is_some() {
                "EMBEDDED".into()
            } else if let Some(p) = &e.usable_path {
                if o.relative_paths {
                    relative(
                        p,
                        doc.path
                            .as_deref()
                            .and_then(Path::parent)
                            .unwrap_or_else(|| Path::new(".")),
                    )
                } else {
                    p.to_string_lossy().into_owned()
                }
            } else {
                e.original_path.clone().unwrap_or_else(|| "(null)".into())
            };
            let metadata = !video && o.metadata && !e.metadata.is_empty();
            writeln!(
                out,
                " {} File: \"{path}\"",
                if metadata { "├──" } else { "└──" }
            )
            .unwrap();
            if metadata {
                out.push_str(" └── Metadata:\n");
                meta(&mut out, &e.metadata, "     ", false);
            }
            out.push('\n');
        }
        if video && c.essences.iter().any(|e| e.format == EssenceFormat::Video) {
            out.push('\n');
        }
    }
    out.push('\n');
    out
}
fn decibels(value: f64) -> String {
    let v = 20.0 * value.log10();
    if v.is_finite() {
        format!("{v:+05.1}")
    } else if v.is_sign_negative() {
        " -inf".into()
    } else {
        "  nan".into()
    }
}
fn gain(g: &Curve) -> String {
    if g.constant {
        format!("{} dB", decibels(g.value()))
    } else {
        "(A)     ".into()
    }
}
fn pan(g: &Curve) -> String {
    if g.constant {
        let v = g.value();
        format!(
            "{v:.2} {} ",
            if v == 0.0 {
                "(L)"
            } else if v == 0.5 {
                "(C)"
            } else if v == 1.0 {
                "(R)"
            } else {
                "   "
            }
        )
    } else {
        "(A)    ".into()
    }
}
fn points(out: &mut String, c: &Curve, db: bool, padding: &str) {
    for (t, v) in &c.points {
        write!(
            out,
            "{padding}_time: {:.6}   _value: {:.6}",
            t.as_f64(),
            v.as_f64()
        )
        .unwrap();
        if db {
            write!(out, " ({} dB)", decibels(v.as_f64())).unwrap();
        }
        out.push('\n');
    }
}
pub fn clips(c: &Composition, o: &DisplayOptions) -> String {
    let mut out = String::from("Tracks & Clips :\n================\n\n");
    for t in c.tracks.iter().filter(|t| t.video) {
        let start = c.timecode.edit_rate.convert(c.timecode.start, t.edit_rate);
        write!(
            out,
            " VideoTrack[{}] ::  EditRate: {}/{} ({:.2})",
            t.number,
            t.edit_rate.numerator,
            t.edit_rate.denominator,
            t.edit_rate.as_f64()
        )
        .unwrap();
        if let Some(name) = &t.name
            && !name.is_empty()
        {
            write!(out, "  Name: \"{name}\"").unwrap();
        }
        out.push('\n');
        if let Some(clip) = t.items.iter().find_map(Item::clip) {
            writeln!(
                out,
                " └── Clip (1): Start: {}  Len: {}  End: {}  SourceOffset: {}\n",
                pos(clip.position + start, t.edit_rate, c, o, None),
                pos(clip.length, t.edit_rate, c, o, None),
                pos(clip.position + start + clip.length, t.edit_rate, c, o, None),
                pos(clip.source_offset, t.edit_rate, c, o, None)
            )
            .unwrap();
        }
        out.push('\n');
    }
    out.push('\n');
    for t in c.tracks.iter().filter(|t| !t.video) {
        let start = c.timecode.edit_rate.convert(c.timecode.start, t.edit_rate);
        let mut counter = 1;
        let total = t.items.iter().filter(|i| i.clip().is_some()).count()
            + [&t.pan, &t.gain]
                .iter()
                .filter(|g| o.automation && g.as_ref().is_some_and(|g| !g.constant))
                .count();
        write!(
            out,
            " AudioTrack[{}] ::  EditRate: {}/{} ({:.2})  Format: {}",
            t.number,
            t.edit_rate.numerator,
            t.edit_rate.denominator,
            t.edit_rate.as_f64(),
            match t.channels {
                1 => "MONO",
                2 => "STEREO",
                6 => "5.1",
                8 => "7.1",
                _ => "Unknwn",
            }
        )
        .unwrap();
        if t.solo {
            out.push_str("  Solo: SOLO");
        }
        if t.mute {
            out.push_str("  Mute: MUTE");
        }
        if let Some(g) = &t.gain {
            write!(out, "  Gain: {}", gain(g)).unwrap();
        }
        if let Some(p) = &t.pan {
            write!(out, "  Pan: {}", pan(p)).unwrap();
        }
        if let Some(name) = &t.name
            && !name.is_empty()
        {
            write!(out, "  Name: \"{name}\"").unwrap();
        }
        out.push('\n');
        let mut clip_index = 0;
        for (i, item) in t.items.iter().enumerate() {
            if let Item::Transition(trans) = item {
                if trans.kind == FadeKind::Cross {
                    writeln!(
                        out,
                        " {} X-FADE: {}  Length: {}  CutPoint: {}",
                        if counter <= total {
                            "├──"
                        } else {
                            "└──"
                        },
                        trans.interpolation.label(),
                        pos(trans.length, t.edit_rate, c, o, None),
                        pos(trans.cut_point, t.edit_rate, c, o, None)
                    )
                    .unwrap();
                    writeln!(out, " {}", if counter <= total { "│" } else { " " }).unwrap();
                }
                continue;
            }
            let clip = item.clip().unwrap();
            clip_index += 1;
            let stem = if counter < total { "│" } else { " " };
            let beyond = clip
                .sources
                .iter()
                .filter_map(|s| c.essences.get(s.essence))
                .any(|e| e.length > 0 && t.edit_rate.convert(clip.length, e.edit_rate) > e.length);
            write!(out," {} Clip ({clip_index}):  Start: {}  Len: {}{}  End: {}  SourceOffset: {}  Channels: {}",if counter<total{"├──"}else{"└──"},pos(clip.position+start,t.edit_rate,c,o,None),pos(clip.length,t.edit_rate,c,o,None),if beyond{" (out of range !)"}else{""},pos(clip.position+start+clip.length,t.edit_rate,c,o,None),pos(clip.source_offset,t.edit_rate,c,o,None),clip.channels).unwrap();
            if clip.mute {
                out.push_str("  Mute: MUTE");
            }
            if let Some(g) = &clip.gain {
                write!(out, "  Gain: {}", gain(g)).unwrap();
            }
            if clip.automation.is_some() {
                out.push_str("  VolumeAutomation: YES");
            }
            for (adj, kind, label) in [
                (i.checked_sub(1), FadeKind::In, "FadeIn"),
                (Some(i + 1), FadeKind::Out, "FadeOut"),
            ] {
                if let Some(Item::Transition(tr)) = adj.and_then(|j| t.items.get(j))
                    && tr.kind == kind
                {
                    write!(
                        out,
                        "  {label}: {} ({})",
                        tr.interpolation.label(),
                        pos(tr.length, t.edit_rate, c, o, None)
                    )
                    .unwrap();
                }
            }
            if let Some(n) = &clip.subclip_name {
                write!(out, "  ClipName: \"{n}\"").unwrap();
            }
            out.push('\n');
            let line_total = clip.sources.len()
                + usize::from(o.metadata && !clip.metadata.is_empty())
                + usize::from(o.automation && clip.automation.is_some());
            let mut line = 1;
            for s in &clip.sources {
                writeln!(
                    out,
                    " {stem}   {} SourceFile [ch {}]: \"{}\"",
                    if line < line_total {
                        "├──"
                    } else {
                        "└──"
                    },
                    if s.channel == 0 {
                        "ALL".into()
                    } else {
                        s.channel.to_string()
                    },
                    c.essences[s.essence].unique_name
                )
                .unwrap();
                line += 1;
            }
            if o.metadata && !clip.metadata.is_empty() {
                writeln!(
                    out,
                    " {stem}   {} Metadata {}",
                    if line < line_total {
                        "├──"
                    } else {
                        "└──"
                    },
                    "─".repeat(36)
                )
                .unwrap();
                let padding = format!(" {stem}   {}   ", if line < line_total { "│" } else { " " });
                meta(&mut out, &clip.metadata, &padding, true);
                line += 1;
            }
            if o.automation
                && let Some(g) = &clip.automation
            {
                writeln!(
                    out,
                    " {stem}   {} Clip Volume Automation {}",
                    if line < line_total {
                        "├──"
                    } else {
                        "└──"
                    },
                    "─".repeat(22)
                )
                .unwrap();
                points(
                    &mut out,
                    g,
                    true,
                    &format!(" {stem}   {}   ", if line < line_total { "│" } else { " " }),
                );
            }
            writeln!(out, " {stem}").unwrap();
            counter += 1;
        }
        if o.automation {
            for (g, label) in [(&t.pan, "Pan"), (&t.gain, "Volume")] {
                if let Some(g) = g
                    && !g.constant
                {
                    writeln!(out, " └── Track {label} Automation {}", "─".repeat(13)).unwrap();
                    points(
                        &mut out,
                        g,
                        false,
                        if counter < total { " │   " } else { "     " },
                    );
                    counter += 1;
                }
            }
        }
        out.push('\n');
    }
    if !c.markers.is_empty() {
        out.push_str("Markers :\n=========\n\n");
    }
    for (i, m) in c.markers.iter().enumerate() {
        let start = c.timecode.edit_rate.convert(c.timecode.start, m.edit_rate);
        writeln!(out," Marker[{i}]:  EditRate: {}/{} ({:.2})  Start: {}  Length: {}  Color: #{:02x}{:02x}{:02x}  Label: \"{}\"  Comment: \"{}\"",m.edit_rate.numerator,m.edit_rate.denominator,m.edit_rate.as_f64(),pos(m.position+start,m.edit_rate,c,o,None),pos(m.length,m.edit_rate,c,o,None),m.color[0],m.color[1],m.color[2],m.name.as_deref().unwrap_or("(null)"),m.comment.as_deref().unwrap_or("(null)")).unwrap();
    }
    out
}
/// Raw bytes are retained even when a property isn't recognized by the dictionary.
pub fn object_properties(doc: &Document, id: ObjectId) -> String {
    let o = doc.object(id);
    let mut s = String::new();
    for p in o.properties().rev() {
        writeln!(
            s,
            " [0x{:04x}] {} ({})",
            p.id,
            doc.property_name(p.id),
            stored_form(p.stored_form)
        )
        .unwrap();
        s.push_str(&hex(o.raw(p.id).unwrap_or(&[]), " "));
        for &target in p.references {
            let target = doc.object(target);
            writeln!(
                s,
                " => node {} @ {}",
                target.node,
                doc.cfb.path(target.node)
            )
            .unwrap();
        }
    }
    s
}
fn stored_form(sf: u16) -> &'static str {
    match sf {
        0x82 => "SF_DATA",
        0x42 => "SF_DATA_STREAM",
        0x22 => "SF_STRONG_OBJECT_REFERENCE",
        0x32 => "SF_STRONG_OBJECT_REFERENCE_VECTOR",
        0x3a => "SF_STRONG_OBJECT_REFERENCE_SET",
        0x2 => "SF_WEAK_OBJECT_REFERENCE",
        0x12 => "SF_WEAK_OBJECT_REFERENCE_VECTOR",
        0x1a => "SF_WEAK_OBJECT_REFERENCE_SET",
        0x3 => "SF_WEAK_OBJECT_REFERENCE_STORED_OBJECT_ID",
        0x86 => "SF_UNIQUE_OBJECT_ID",
        0x40 => "SF_OPAQUE_STREAM",
        _ => "UNKNOWN",
    }
}
pub fn hex(bytes: &[u8], padding: &str) -> String {
    let mut s = format!(
        "{padding}______________________________ Hex Dump ______________________________\n\n"
    );
    for chunk in bytes.chunks(16) {
        let mut h = String::new();
        let mut ascii = String::new();
        for (i, b) in chunk.iter().enumerate() {
            write!(h, "{b:02x}{}", if i == 7 { "  " } else { " " }).unwrap();
            if i == 8 {
                ascii.push(' ');
            }
            ascii.push(if b.is_ascii_alphanumeric() {
                *b as char
            } else {
                '.'
            });
        }
        h.truncate(48);
        writeln!(s, "{padding}{h:<48}  |  {ascii}").unwrap();
    }
    write!(
        s,
        "{padding}______________________________________________________________________\n\n"
    )
    .unwrap();
    s
}

pub fn cfb_header(c: &crate::cfb::CompoundFile) -> String {
    let h = &c.header;
    format!(
        " _abSig              : 0xe11ab1a1e011cfd0\n _clsId              : {}\n _version            : {}.{} ( 0x{:04x} 0x{:04x} )\n _uByteOrder         : little-endian ( 0xfffe )\n _uSectorShift       : {} ({} bytes sectors)\n _uMiniSectorShift   : 6 (64 bytes mini-sectors)\n _usReserved0        : 0x00\n _ulReserved1        : 0x0000\n _csectDir           : {}\n _csectFat           : {}\n _sectDirStart       : {}\n _signature          : {}\n _ulMiniSectorCutoff : {}\n _sectMiniFatStart   : {}\n _csectMiniFat       : {}\n _sectDifStart       : {}\n _csectDif           : {}\n\n",
        uid_text(h.class),
        h.minor,
        h.major,
        h.minor,
        h.major,
        h.sector_size.trailing_zeros(),
        h.sector_size,
        h.directory_sectors,
        h.fat_sectors,
        h.first_directory,
        h.transaction,
        h.mini_cutoff,
        h.first_minifat,
        h.minifat_sectors,
        if h.difat_sectors == 0 {
            0xffff_fffe
        } else {
            h.first_difat
        },
        h.difat_sectors
    )
}
fn width(count: usize) -> usize {
    (count.saturating_sub(1)).checked_ilog10().unwrap_or(0) as usize + 1
}
pub fn allocation(c: &crate::cfb::CompoundFile, kind: &str) -> String {
    let mut difat = Vec::new();
    let table = match kind {
        "FAT" => &c.fat,
        "MiniFAT" => &c.minifat,
        _ => {
            difat.extend(c.difat.iter().copied().take(109));
            difat.resize(109, 0xffff_ffff);
            difat.extend(c.difat.iter().copied().skip(109));
            &difat
        }
    };
    let label = format!("_CFB_{kind}");
    let mut s = format!("{label}{}\n\n", "_".repeat(95 - label.len()));
    let w = width(if kind == "DiFAT" {
        c.minifat.len()
    } else {
        table.len()
    });
    for (i, &v) in table.iter().enumerate() {
        let tag = match v {
            0xffff_fffa => "(CFB_MAX_REG_SECT)",
            0xffff_fffc => "(CFB_DIFAT_SECT)",
            0xffff_fffd => "(CFB_FAT_SECT)",
            0xffff_fffe => "(CFB_END_OF_CHAIN)",
            0xffff_ffff => "(CFB_FREE_SECT)",
            _ => "",
        };
        writeln!(s, " SECT[{i:0w$}] : 0x{v:08x} {tag}").unwrap();
    }
    write!(
        s,
        "\n End of {kind}.\n\n Total {kind} entries   : {}\n",
        table.len()
    )
    .unwrap();
    match kind {
        "FAT" => writeln!(s, " Count of FAT sector : {}", c.header.fat_sectors).unwrap(),
        "MiniFAT" => write!(
            s,
            " First MiniFAT sector ID : {}\n Count of MiniFAT sector : {}\n",
            c.header.first_minifat, c.header.minifat_sectors
        )
        .unwrap(),
        _ => write!(
            s,
            " First DiFAT sector ID : {}\n Count of DiFAT sector : Header + {}\n",
            if c.header.difat_sectors == 0 {
                0xffff_fffe
            } else {
                c.header.first_difat
            },
            c.header.difat_sectors
        )
        .unwrap(),
    };
    s.push_str("\n\n");
    s
}
pub fn nodes(c: &crate::cfb::CompoundFile) -> String {
    let mut s = String::new();
    let mut stack = vec![0u32];
    let mut i = 0;
    let w = width(c.nodes.len());
    while let Some(id) = stack.pop() {
        let n = &c.nodes.get(id as usize).unwrap();
        writeln!(s, " {i:0w$} : (null){}", c.path(id)).unwrap();
        i += 1;
        for v in [n.right, n.left, n.child] {
            if (v as i32) > 0 {
                stack.push(v);
            }
        }
    }
    s.push_str("\n\n");
    s
}
pub fn node(c: &crate::cfb::CompoundFile, id: u32) -> String {
    let Some(n) = c.nodes.get(id as usize) else {
        return String::new();
    };
    let mut s = format!(
        "\n _ab          : {}\n _cb          : {}\n _mse         : {}\n _bflags      : {}\n _sidLeftSib  : 0x{:08x}\n _sidRightSib : 0x{:08x}\n",
        n.name,
        n.name.encode_utf16().count() * 2 + 2,
        match n.kind {
            1 => "STGTY_STORAGE",
            2 => "STGTY_STREAM",
            5 => "STGTY_ROOT",
            _ => "STGTY_INVALID",
        },
        if n.color == 1 { "BLACK" } else { "RED" },
        n.left,
        n.right
    );
    if n.kind == 1 || n.kind == 5 {
        write!(
            s,
            " _sidChild    : 0x{:08x}\n _clsid       : {}\n _dwUserFlags : 0x{:08x} ({})\n",
            n.child,
            uid_text(n.class),
            n.state,
            n.state as i32
        )
        .unwrap();
    }
    if n.kind == 2 || n.kind == 5 {
        write!(s," _sectStart   : 0x{:08x} ({})\n _ulSizeLow   : 0x{:08x} ({})\n _ulSizeHigh  : 0x{:08x} ({})\n",n.start,n.start as i32,n.size as u32,n.size as u32 as i32,n.size>>32,n.size>>32).unwrap();
    }
    s.push_str("\n\n");
    s
}
pub fn classes(doc: &Document) -> String {
    let mut s = String::new();
    for c in doc.classes.iter().rev() {
        let mut id = Some(c.id);
        while let Some(uid) = id {
            write!(s, " {}", doc.class_name(uid)).unwrap();
            id = doc.class(uid).and_then(|c| c.parent);
            if id.is_some() {
                s.push_str(" > ");
            }
        }
        s.push('\n');
    }
    s.push_str("\n\n\n\n");
    s
}
pub fn dictionary(doc: &Document) -> String {
    let mut s = String::new();
    for c in doc.classes.iter().rev() {
        let start = s.len();
        for p in c.properties.iter().rev().filter(|p| p.custom || c.custom) {
            writeln!(
                s,
                " {}::{} (0x{:04x})",
                if c.custom {
                    c.name.clone()
                } else {
                    doc.class_name(c.id)
                },
                p.name,
                p.id
            )
            .unwrap();
        }
        if s.len() > start {
            s.push('\n');
        }
    }
    s.push_str("\n\n\n\n");
    s
}
pub fn raw_properties(doc: &Document, b: &[u8], padding: &str) -> crate::Result<String> {
    slice(b, 0, 4)?;
    let count = u16_at(b, 2)? as usize;
    slice(b, 4, count * 6)?;
    let mut offset = 4 + count * 6;
    let mut s = format!(
        "{padding}## Property_Header____________________________________________________\n\n{padding}_byteOrder     : 0x{:02x}\n{padding}_formatVersion : 0x{:02x}\n{padding}_entryCount    : {count}\n\n{padding}======================================================================\n\n\n\n",
        b[0], b[1]
    );
    for i in 0..count {
        let at = 4 + i * 6;
        let pid = u16_at(b, at)?;
        let len = u16_at(b, at + 4)? as usize;
        write!(s,"{padding}#{i} Property_Entry_____________________________________________________\n{padding}_pid        : 0x{pid:04x} ({})\n{padding}_storedForm : {}\n{padding}_length     : {len} bytes\n",doc.property_name(pid),stored_form(u16_at(b,at+2)?)).unwrap();
        s += &hex(slice(b, offset, len)?, padding);
        s.push('\n');
        offset += len;
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timecode_prefers_supported_edit_rate_over_nominal_field() {
        let tc = Timecode {
            start: 0,
            fps: 30,
            drop: false,
            edit_rate: Rational::new(24, 1),
        };
        assert_eq!(timecode(86400, Rational::new(24, 1), &tc), "01:00:00:00");
        let tc = Timecode { drop: true, ..tc };
        assert_eq!(timecode(86400, Rational::new(24, 1), &tc), "01:00:00:00");
        let tc = Timecode {
            fps: 30,
            drop: false,
            edit_rate: Rational::new(30026, 1000),
            ..tc
        };
        assert_eq!(timecode(108000, Rational::new(30, 1), &tc), "01:00:00:00");
    }
}
