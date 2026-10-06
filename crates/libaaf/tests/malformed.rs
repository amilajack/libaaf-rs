use libaaf::{
    Document, ReaderOptions,
    cfb::CompoundFile,
    defs::*,
    media::{self, ExtractFormat, FrameRange},
};
use std::{fs, path::PathBuf};
fn fixture() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tooling/reference/test/aaf/PR_WAV_Internal.aaf"),
    )
    .unwrap()
}
#[test]
fn reject_bad_headers_and_truncation() {
    let b = fixture();
    for n in [0, 1, 8, 511, 512, b.len() - 1] {
        assert!(Document::from_bytes(b[..n].to_vec(), ReaderOptions::default()).is_err());
    }
    for (at, value) in [(0, 0), (28, 0), (30, 63), (32, 63)] {
        let mut data = b.clone();
        data[at] = value;
        assert!(Document::from_bytes(data, ReaderOptions::default()).is_err());
    }
}
#[test]
fn reject_invalid_directory_offsets_and_cycles() {
    let b = fixture();
    let c = CompoundFile::from_bytes(b.clone()).unwrap();
    let sector = c.header.sector_size;
    let dir = (c.header.first_directory as usize + 1) * sector;
    let mut bad = b.clone();
    bad[dir + 76..dir + 80].copy_from_slice(&u32::MAX.to_le_bytes()); // Empty directory cannot have AAF root references.
    assert!(Document::from_bytes(bad, ReaderOptions::default()).is_err());
    let mut bad = b.clone();
    bad[dir + 76..dir + 80].copy_from_slice(&0u32.to_le_bytes());
    assert!(CompoundFile::from_bytes(bad).is_err());
    let mut bad = b;
    bad[48..52].copy_from_slice(&0xffff_ff00u32.to_le_bytes());
    assert!(CompoundFile::from_bytes(bad).is_err());
}
#[test]
fn malformed_properties_and_deterministic_mutations_do_not_panic() {
    let b = fixture();
    for index in (0..b.len()).step_by(4093).take(200) {
        let mut data = b.clone();
        data[index] ^= 0xff;
        if let Ok(d) = Document::from_bytes(data, ReaderOptions::default()) {
            let _ = d.composition();
        }
    }
}
#[test]
fn extraction_validates_ranges_before_writing() {
    let d = Document::from_bytes(fixture(), ReaderOptions::default()).unwrap();
    let c = d.composition().unwrap();
    let mut out = Vec::new();
    assert!(
        media::extract_essence(
            &d,
            &c,
            0,
            ExtractFormat::Wave,
            Some(FrameRange {
                offset: u64::MAX,
                length: 1
            }),
            &mut out
        )
        .is_err()
    );
    assert!(out.is_empty());
    assert!(
        media::extract_essence(
            &d,
            &c,
            0,
            ExtractFormat::Wave,
            Some(FrameRange {
                offset: 0,
                length: u64::MAX
            }),
            &mut out
        )
        .is_err()
    );
    assert!(out.is_empty());
    assert!(
        media::extract_essence(
            &d,
            &c,
            0,
            ExtractFormat::Wave,
            Some(FrameRange {
                offset: 0,
                length: 1_000_000
            }),
            &mut out
        )
        .is_err()
    );
    assert!(out.is_empty());
    let path = std::env::temp_dir().join(format!(
        "libaaf-invalid-extraction-{}.wav",
        std::process::id()
    ));
    let invalid = Some(FrameRange {
        offset: u64::MAX,
        length: 1,
    });
    fs::write(&path, b"keep existing audio").unwrap();
    assert!(media::extract_to_file(&d, &c, 0, ExtractFormat::Wave, invalid, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"keep existing audio");
    fs::remove_file(&path).unwrap();
    assert!(media::extract_to_file(&d, &c, 0, ExtractFormat::Wave, invalid, &path).is_err());
    assert!(!path.exists());
    let mut c = c;
    c.essences[0].format = libaaf::timeline::EssenceFormat::Aiff;
    c.essences[0].sample_size = 80;
    assert!(media::extract_essence(&d, &c, 0, ExtractFormat::Wave, None, &mut out).is_err());
    assert!(out.is_empty());
}
#[test]
fn unicode_paths_missing_media_and_unknown_properties() {
    assert_eq!(
        media::decode_uri("file:///M%C3%A9dia/%E8%8A%B1.wav").unwrap(),
        "/Média/花.wav"
    );
    assert!(media::decode_uri("file:///bad%ff").is_err());
    assert!(
        media::resolve_media(
            "/not-present-8cb4bf4a/花.wav",
            std::path::Path::new("."),
            None
        )
        .unwrap()
        .is_none()
    );
    let d = Document::from_bytes(fixture(), ReaderOptions::default()).unwrap();
    let h = d.object(d.header);
    assert_eq!(h.raw(PID_Header_ByteOrder), Some(&[0x49, 0x49][..]));
    for o in d.objects() {
        for p in o.properties() {
            assert_eq!(o.raw(p.id).unwrap().len(), p.bytes.len());
        }
    }
}
fn property_stream_offset(d: &Document, id: libaaf::ObjectId, data: &[u8]) -> usize {
    let o = d.object(id);
    let node = d.cfb.child(o.node, "properties").unwrap();
    let b = d.cfb.stream(node).unwrap().to_vec().unwrap();
    data.windows(b.len())
        .position(|s| s == b)
        .expect("fixture property stream is contiguous")
}
#[test]
fn retain_unknown_properties_and_detect_source_cycles() {
    let mut b = fixture();
    let d = Document::from_bytes(b.clone(), ReaderOptions::default()).unwrap();
    let at = property_stream_offset(&d, d.header, &b);
    let h = d.object(d.header);
    let index = h
        .properties()
        .position(|p| p.id == PID_Header_LastModified)
        .unwrap();
    let expected = h.raw(PID_Header_LastModified).unwrap().to_vec();
    b[at + 4 + index * 6..at + 6 + index * 6].copy_from_slice(&0xf001u16.to_le_bytes());
    let d = Document::from_bytes(b, ReaderOptions::default()).unwrap();
    assert_eq!(d.object(d.header).raw(0xf001).unwrap(), expected);
    let mut b = fixture();
    let d = Document::from_bytes(b.clone(), ReaderOptions::default()).unwrap();
    let c = d.composition().unwrap();
    let clip = c
        .tracks
        .iter()
        .flat_map(|t| &t.items)
        .find_map(|i| i.clip())
        .unwrap();
    let mut parent = d.object(clip.object).parent.unwrap();
    while d.object(parent).class != AAFClassID_TimelineMobSlot {
        parent = d.object(parent).parent.unwrap();
    }
    let slot = d.object(parent).u32(PID_MobSlot_SlotID).unwrap();
    let mob = d.object(parent).parent.unwrap();
    let object = d.object(clip.object);
    let at = property_stream_offset(&d, clip.object, &b);
    let p = object.property(PID_SourceReference_SourceID).unwrap();
    b[at + p.bytes.start..at + p.bytes.end]
        .copy_from_slice(d.object(mob).raw(PID_Mob_MobID).unwrap());
    let p = object
        .property(PID_SourceReference_SourceMobSlotID)
        .unwrap();
    b[at + p.bytes.start..at + p.bytes.end].copy_from_slice(&slot.to_le_bytes());
    let d = Document::from_bytes(b, ReaderOptions::default()).unwrap();
    assert!(matches!(d.composition(), Err(libaaf::Error::Cycle { .. })));
}

#[test]
fn malformed_files_match_owned_input_rejection() {
    let original = fixture();
    let path = std::env::temp_dir().join(format!("libaaf-native-雪-{}.aaf", std::process::id()));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    drop(file);
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let mut cases = Vec::new();
    for len in [0, 8, 511, 512, original.len() - 1] {
        cases.push(original[..len].to_vec());
    }
    for (offset, value) in [(0, 0), (28, 0), (30, 63), (32, 63), (48, 0xff)] {
        let mut data = original.clone();
        data[offset] = value;
        cases.push(data);
    }
    for data in cases {
        fs::write(&path, &data).unwrap();
        assert!(Document::from_bytes(data, ReaderOptions::default()).is_err());
        assert!(Document::open(&path, ReaderOptions::default()).is_err());
    }
}

#[test]
fn reject_oversized_property_stream_before_allocating_arena() {
    let mut bytes = fixture();
    let cfb = CompoundFile::from_bytes(bytes.clone()).unwrap();
    let node = cfb.child(0, "properties").unwrap() as usize;
    let per_sector = cfb.header.sector_size / 128;
    let mut sector = cfb.header.first_directory;
    for _ in 0..node / per_sector {
        sector = cfb.fat[sector as usize];
    }
    let offset = (sector as usize + 1) * cfb.header.sector_size + (node % per_sector) * 128 + 120;
    bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        Document::from_bytes(bytes, ReaderOptions::default()),
        Err(libaaf::Error::Invalid {
            context: "metadata stream sizes",
            ..
        })
    ));
}

#[test]
fn reject_pcm_descriptor_truncation_and_fractional_rates() {
    let bytes = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tooling/reference/test/aaf/PT_PCM_Internal.aaf"),
    )
    .unwrap();
    let doc = Document::from_bytes(bytes.clone(), ReaderOptions::default()).unwrap();
    let descriptor = doc
        .objects()
        .find(|o| o.class == AAFClassID_PCMDescriptor)
        .unwrap();
    let stream = doc
        .cfb
        .stream(doc.cfb.child(descriptor.node, "properties").unwrap())
        .unwrap()
        .to_vec()
        .unwrap();
    let positions: Vec<_> = bytes
        .windows(stream.len())
        .enumerate()
        .filter(|(_, b)| *b == stream)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        positions.len(),
        1,
        "fixture descriptor must be contiguous and unique"
    );
    for (pid, replacement) in [
        (
            PID_SoundDescriptor_Channels,
            65536u32.to_le_bytes().to_vec(),
        ),
        (
            PID_SoundDescriptor_QuantizationBits,
            65535u32.to_le_bytes().to_vec(),
        ),
        (
            PID_FileDescriptor_SampleRate,
            [48000i32.to_le_bytes(), 2i32.to_le_bytes()].concat(),
        ),
        (
            PID_FileDescriptor_SampleRate,
            [(-48000i32).to_le_bytes(), 1i32.to_le_bytes()].concat(),
        ),
    ] {
        let range = descriptor.property(pid).unwrap().bytes;
        let mut modified = bytes.clone();
        modified[positions[0] + range.start..positions[0] + range.end]
            .copy_from_slice(&replacement);
        let doc = Document::from_bytes(modified, ReaderOptions::default()).unwrap();
        assert!(doc.composition().is_err(), "invalid PCM property {pid:04x}");
    }
}

#[test]
fn reject_media_rates_that_cannot_fit_aaf_rationals() {
    let doc = Document::from_bytes(fixture(), ReaderOptions::default()).unwrap();
    let composition = doc.composition().unwrap();
    let node = composition
        .essences
        .iter()
        .find_map(|e| e.embedded_node)
        .unwrap();
    let mut wave = doc.cfb.stream(node).unwrap().to_vec().unwrap();
    let fmt = wave.windows(4).position(|b| b == b"fmt ").unwrap();
    for rate in [0, i32::MAX as u32, u32::MAX] {
        wave[fmt + 12..fmt + 16].copy_from_slice(&rate.to_le_bytes());
        assert!(
            media::audio_info(&wave).is_err(),
            "invalid sample rate {rate}"
        );
    }
    wave[fmt + 12..fmt + 16].copy_from_slice(&48000u32.to_le_bytes());
    assert_eq!(media::audio_info(&wave).unwrap().sample_rate, 48000);
}

#[test]
fn unnamed_top_level_composition_is_not_parsed_twice() {
    let mut bytes = fixture();
    let doc = Document::from_bytes(bytes.clone(), ReaderOptions::default()).unwrap();
    let mob = doc
        .object(doc.content)
        .references(PID_ContentStorage_Mobs)
        .iter()
        .map(|&id| doc.object(id))
        .find(|o| o.class == AAFClassID_CompositionMob)
        .unwrap();
    let name = doc
        .object(doc.content)
        .string(PID_ContentStorage_Mobs)
        .unwrap();
    let index = doc
        .cfb
        .child(doc.object(doc.content).node, &format!("{name} index"))
        .unwrap();
    let index_node = index;
    let mut index = doc.cfb.stream(index_node).unwrap().to_vec().unwrap();
    let count = u32::from_le_bytes(index[..4].try_into().unwrap()) as usize;
    let stride = 8 + index[14] as usize;
    let first_entry = (0..count)
        .map(|i| 15 + i * stride)
        .find(|&at| {
            let key = u32::from_le_bytes(index[at..at + 4].try_into().unwrap());
            doc.cfb
                .child(doc.object(doc.content).node, &format!("{name}{{{key:x}}}"))
                == Some(mob.node)
        })
        .unwrap();
    assert!(first_entry > 15, "set iteration must have a later entry");
    let props_node = doc.cfb.child(mob.node, "properties").unwrap();
    let mut props = doc.cfb.stream(props_node).unwrap().to_vec().unwrap();
    let count = u16::from_le_bytes(props[2..4].try_into().unwrap()) as usize;
    let name_entry = (0..count)
        .map(|i| 4 + i * 6)
        .find(|&at| props[at..at + 2] == PID_Mob_Name.to_le_bytes())
        .unwrap();
    // A malformed set repeats the composition after its first occurrence.
    // Even without a name it must contribute its tracks only once.
    index.copy_within(first_entry..first_entry + stride, first_entry - stride);
    overwrite_fixture_stream(&doc.cfb, &mut bytes, index_node, &index);
    let named = Document::from_bytes(bytes.clone(), ReaderOptions::default()).unwrap();
    let expected = named.composition().unwrap();
    assert!(!expected.tracks.is_empty());
    props[name_entry..name_entry + 2].copy_from_slice(&0xfffeu16.to_le_bytes());
    overwrite_fixture_stream(&doc.cfb, &mut bytes, props_node, &props);
    let unnamed = Document::from_bytes(bytes, ReaderOptions::default()).unwrap();
    let actual = unnamed.composition().unwrap();
    assert!(actual.name.is_none());
    assert_eq!(actual.tracks.len(), expected.tracks.len());
    assert_eq!(actual.essences.len(), expected.essences.len());
    for (actual, expected) in actual.tracks.iter().zip(&expected.tracks) {
        assert_eq!(actual.items.len(), expected.items.len());
    }
    assert!(
        actual
            .diagnostics
            .iter()
            .any(|d| { d.message == "multiple top-level compositions unsupported" })
    );
}

// Test-only in-place edits preserve the CFB allocation chains and stream sizes.
fn overwrite_fixture_stream(cfb: &CompoundFile, bytes: &mut [u8], node: u32, data: &[u8]) {
    let node = cfb.nodes.get(node as usize).unwrap();
    assert_eq!(node.size as usize, data.len());
    let sector_size = cfb.header.sector_size;
    let mini = node.kind != 5 && node.size < u64::from(cfb.header.mini_cutoff);
    let unit = if mini { 64 } else { sector_size };
    let mut sid = node.start;
    for chunk in data.chunks(unit) {
        let offset = if mini {
            let logical = sid as usize * 64;
            let mut root_sector = cfb.nodes.get(0).unwrap().start;
            for _ in 0..logical / sector_size {
                root_sector = cfb.fat[root_sector as usize];
            }
            (root_sector as usize + 1) * sector_size + logical % sector_size
        } else {
            (sid as usize + 1) * sector_size
        };
        bytes[offset..offset + chunk.len()].copy_from_slice(chunk);
        sid = if mini {
            cfb.minifat[sid as usize]
        } else {
            cfb.fat[sid as usize]
        };
    }
}
