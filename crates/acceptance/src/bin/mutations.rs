//! Development-only fixture perturbations; this is not an AAF writing API.
use libaaf::{Document, ReaderOptions, defs::*};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: mutations <AAF> <output-directory>".into());
    }
    let bytes = fs::read(&args[1])?;
    let doc = Document::from_bytes(bytes.clone(), ReaderOptions::default())?;
    let output = Path::new(&args[2]);
    fs::create_dir_all(output)?;
    // Keep the CFB stream size/layout intact while moving WAV data past 64 KiB.
    // Both readers must walk the JUNK chunk rather than inspect a fixed prefix.
    let composition = doc.composition()?;
    if let Some(essence) = composition
        .essences
        .iter()
        .find(|e| e.format == libaaf::timeline::EssenceFormat::Wave && e.embedded_node.is_some())
    {
        let stream = doc.cfb.stream(essence.embedded_node.unwrap())?.to_vec()?;
        let mut positions = bytes
            .windows(stream.len())
            .enumerate()
            .filter(|(_, b)| *b == stream)
            .map(|(i, _)| i);
        if let Some(at) = positions.next().filter(|_| positions.next().is_none()) {
            let block = essence.channels * (essence.sample_size / 8);
            let mut wave = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            wave.extend(16u32.to_le_bytes());
            wave.extend(1u16.to_le_bytes());
            wave.extend(essence.channels.to_le_bytes());
            wave.extend(essence.sample_rate.to_le_bytes());
            wave.extend((essence.sample_rate * u32::from(block)).to_le_bytes());
            wave.extend(block.to_le_bytes());
            wave.extend(essence.sample_size.to_le_bytes());
            wave.extend(b"JUNK");
            wave.extend(70000u32.to_le_bytes());
            wave.resize(wave.len() + 70000, 0);
            wave.extend(b"data");
            wave.extend((u32::from(block) * 256).to_le_bytes());
            wave.extend((0..usize::from(block) * 256).map(|i| (i % 251) as u8));
            let riff_size = u32::try_from(wave.len() - 8)?;
            wave[4..8].copy_from_slice(&riff_size.to_le_bytes());
            if wave.len() <= stream.len() {
                wave.resize(stream.len(), 0);
                let mut modified = bytes.clone();
                modified[at..at + stream.len()].copy_from_slice(&wave);
                fs::write(output.join("large-wave-header.aaf"), &modified)?;
                let descriptor = doc.object(essence.descriptor);
                if let Some(property) = descriptor.property(PID_WAVEDescriptor_Summary) {
                    let summary = descriptor.raw(PID_WAVEDescriptor_Summary).unwrap();
                    let props_node = doc.cfb.child(descriptor.node, "properties").unwrap();
                    let props = doc.cfb.stream(props_node)?.to_vec()?;
                    let mut locations = bytes
                        .windows(props.len())
                        .enumerate()
                        .filter(|(_, b)| *b == props)
                        .map(|(i, _)| i);
                    if let (Some(base), Some(data)) = (
                        locations.next(),
                        summary.windows(4).position(|b| b == b"data"),
                    ) && locations.next().is_none()
                        && data + 8 <= summary.len()
                    {
                        modified[base + property.bytes.start + data + 4
                            ..base + property.bytes.start + data + 8]
                            .fill(0);
                        fs::write(output.join("large-wave-header-fallback.aaf"), modified)?;
                    }
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for object in doc.objects() {
        // Hide a required property by renaming its table entry, keeping its
        // original payload and the complete compound-file layout intact.
        let omitted = if object.class == AAFClassID_VaryingValue {
            Some(PID_VaryingValue_Interpolation)
        } else if object.class == AAFClassID_ControlPoint {
            Some(PID_ControlPoint_Time)
        } else {
            None
        };
        if let Some(pid) = omitted
            && !seen.contains(&(object.class, pid))
            && object.property(pid).is_some()
            && object.property(0xfffe).is_none()
            && let Some(node) = doc.cfb.child(object.node, "properties")
        {
            let stream = doc.cfb.stream(node)?.to_vec()?;
            let locations: Vec<_> = bytes
                .windows(stream.len())
                .enumerate()
                .filter(|(_, b)| *b == stream)
                .map(|(i, _)| i)
                .collect();
            if let [base] = locations.as_slice() {
                let count = u16::from_le_bytes([stream[2], stream[3]]) as usize;
                let entry = (0..count)
                    .map(|i| 4 + 6 * i)
                    .find(|&at| stream[at..at + 2] == pid.to_le_bytes())
                    .unwrap();
                let mut modified = bytes.clone();
                modified[base + entry..base + entry + 2].copy_from_slice(&0xfffeu16.to_le_bytes());
                fs::write(
                    output.join(format!(
                        "{}-{pid:04x}-missing.aaf",
                        doc.class_name(object.class)
                    )),
                    modified,
                )?;
                seen.insert((object.class, pid));
            }
        }
        let mut changes = Vec::new();
        for (class, pid, values) in [
            (
                AAFClassID_SourceClip,
                PID_SourceClip_StartTime,
                &[0i64, 1][..],
            ),
            (AAFClassID_Timecode, PID_Timecode_Start, &[0, 100][..]),
            (AAFClassID_Timecode, PID_Timecode_FPS, &[24, 30][..]),
            (AAFClassID_Transition, PID_Transition_CutPoint, &[0, 1][..]),
        ] {
            if object.class != class {
                continue;
            }
            if let Some(raw) = object.raw(pid).filter(|b| matches!(b.len(), 2 | 8)) {
                changes.push((
                    pid,
                    values
                        .iter()
                        .map(|v| (v.to_string(), v.to_le_bytes()[..raw.len()].to_vec()))
                        .collect::<Vec<_>>(),
                ));
            }
        }
        if object.class == AAFClassID_ConstantValue || object.class == AAFClassID_VaryingValue {
            changes.push((
                PID_Parameter_Definition,
                [
                    ("amplitude", AAFParameterDef_Amplitude),
                    ("pan", AAFParameterDef_Pan),
                    ("level", AAFParameterDef_Level),
                ]
                .map(|(name, uid)| (name.to_owned(), uid.0.to_vec()))
                .to_vec(),
            ));
        }
        if object.class == AAFClassID_ConstantValue
            && object
                .indirect(PID_ConstantValue_Value)
                .is_some_and(|(t, b)| t == AAFTypeID_Rational && b.len() == 8)
        {
            let raw = object.raw(PID_ConstantValue_Value).unwrap();
            changes.push((
                PID_ConstantValue_Value,
                [0i32, 1, 2]
                    .map(|numerator| {
                        let mut data = raw.to_vec();
                        data[17..21].copy_from_slice(&numerator.to_le_bytes());
                        data[21..25].copy_from_slice(&2i32.to_le_bytes());
                        (format!("rational-{numerator}"), data)
                    })
                    .to_vec(),
            ));
        }
        if object.class == AAFClassID_CompositionMob
            && object.uid(PID_Mob_UsageCode) == Some(AAFUsage_AdjustedClip)
        {
            changes.push((
                PID_Mob_UsageCode,
                vec![("subclip".into(), AAFUsage_SubClip.0.to_vec())],
            ));
        }
        if object.class == AAFClassID_ControlPoint
            && let Some(raw) = object.raw(PID_ControlPoint_Value).filter(|b| b.len() >= 17)
        {
            let mut wrong_type = raw.to_vec();
            wrong_type[1..17].copy_from_slice(&AAFTypeID_Boolean.0);
            changes.push((
                PID_ControlPoint_Value,
                vec![("wrong-type".into(), wrong_type)],
            ));
        }
        if object.class == AAFClassID_TaggedValue
            && object.string(PID_TaggedValue_Name).as_deref() == Some("_DISABLE_CLIP_FLAG")
        {
            let name = "_UNKNOWN_CLIP_FLAG\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            changes.push((PID_TaggedValue_Name, vec![("unknown-flag".into(), name)]));
        }
        for (pid, variants) in changes {
            if seen.contains(&(object.class, pid)) {
                continue;
            }
            let Some(property) = object.property(pid) else {
                continue;
            };
            let Some(node) = doc.cfb.child(object.node, "properties") else {
                continue;
            };
            let stream = doc.cfb.stream(node)?.to_vec()?;
            // Only mutate uniquely located contiguous streams. Fragmented streams
            // remain covered by normal fixtures and parser fuzzing.
            let mut positions = bytes
                .windows(stream.len())
                .enumerate()
                .filter(|(_, b)| *b == stream)
                .map(|(i, _)| i);
            let Some(at) = positions.next() else {
                continue;
            };
            if positions.next().is_some() {
                continue;
            }
            seen.insert((object.class, pid));
            for (name, replacement) in variants {
                if replacement.len() != property.bytes.len() {
                    continue;
                }
                let mut modified = bytes.clone();
                modified[at + property.bytes.start..at + property.bytes.end]
                    .copy_from_slice(&replacement);
                fs::write(
                    output.join(format!(
                        "{}-{pid:04x}-{name}.aaf",
                        doc.class_name(object.class)
                    )),
                    modified,
                )?;
            }
        }
    }
    Ok(())
}
