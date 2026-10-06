use libaaf::{Document, ReaderOptions};
use std::path::PathBuf;
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tooling/reference/test/aaf")
}
#[test]
fn decode_complete_fixture_suite() {
    let mut count = 0;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "aaf") {
            let d = Document::open(&path, ReaderOptions::default())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(d.objects().len() > 0);
            let c = d
                .composition()
                .unwrap_or_else(|e| panic!("{} timeline: {e}", path.display()));
            assert!(c.name.is_some());
            count += 1;
        }
    }
    assert_eq!(count, 65);
}

#[test]
fn file_and_owned_streams_match() {
    use libaaf::cfb::CompoundFile;
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "aaf") {
            continue;
        }
        let disk = CompoundFile::open(&path).unwrap();
        let owned = CompoundFile::from_bytes(std::fs::read(&path).unwrap()).unwrap();
        for node in disk.nodes.iter().filter(|n| n.kind == 2 || n.kind == 5) {
            let expected = owned.stream(node.id).unwrap().to_vec().unwrap();
            let actual = disk.stream(node.id).unwrap();
            assert_eq!(
                actual.to_vec().unwrap(),
                expected,
                "{} node {}",
                path.display(),
                node.id
            );
            let mut copied = Vec::new();
            actual.copy_to(&mut copied).unwrap();
            assert_eq!(copied, expected);
            if expected.len() > 4 {
                copied.clear();
                actual
                    .copy_range(1, expected.len() - 3, &mut copied)
                    .unwrap();
                assert_eq!(copied, expected[1..expected.len() - 2]);
            }
        }
    }
}
