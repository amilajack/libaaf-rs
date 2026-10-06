use libaaf::{
    Document, Rational, ReaderOptions, inspection,
    timeline::{FadeKind, Item, Timecode},
};
use std::path::PathBuf;

#[test]
fn expose_edit_protocol_fade_endpoints_from_real_sessions() {
    let mut kinds = [false; 3];
    for name in ["MC_Fades.aaf", "PR_Fades.aaf", "PT_Fades.aaf"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tooling/reference/test/aaf")
            .join(name);
        let d = Document::open(
            path,
            ReaderOptions {
                recover_protools_fades: true,
                ..Default::default()
            },
        )
        .unwrap();
        for t in &d.composition().unwrap().tracks {
            for item in &t.items {
                let Item::Transition(t) = item else { continue };
                let curve = t.level_curve();
                let values = if t.kind == FadeKind::Out {
                    [1, 0]
                } else {
                    [0, 1]
                };
                assert_eq!(curve.interpolation, t.interpolation);
                assert_eq!(
                    curve.points,
                    vec![
                        (Rational::new(0, 1), Rational::new(values[0], 1)),
                        (Rational::new(1, 1), Rational::new(values[1], 1))
                    ]
                );
                kinds[match t.kind {
                    FadeKind::In => 0,
                    FadeKind::Out => 1,
                    FadeKind::Cross => 2,
                }] = true;
            }
        }
    }
    assert_eq!(kinds, [true; 3]);
}

#[test]
fn timecode_formats_full_signed_range_without_overflow() {
    for fps in [1, 25, 30, 60, 120] {
        for drop in [false, true] {
            let tc = Timecode {
                start: 0,
                fps,
                drop,
                edit_rate: Rational::new(fps as i32, 1),
            };
            assert!(inspection::timecode(i64::MIN, tc.edit_rate, &tc).starts_with('-'));
            assert!(!inspection::timecode(i64::MAX, tc.edit_rate, &tc).starts_with('-'));
        }
    }
}
