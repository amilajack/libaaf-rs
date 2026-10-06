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
