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
