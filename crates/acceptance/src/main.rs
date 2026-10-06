//! Development-only paired benchmark counterpart to tooling/benchmark.c.
use libaaf::{
    Document, ReaderOptions,
    media::{ExtractFormat, extract_to_file, extraction_filename},
};
use std::{hint::black_box, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err(
            "usage: libaaf-acceptance <parse|extract|profile> <file> <iterations-or-profile-seconds> <output-directory>"
                .into(),
        );
    }
    let iterations: usize = args[3].parse()?;
    let output = PathBuf::from(&args[4]);
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let elapsed = match args[1].as_str() {
        "parse" => {
            let start = Instant::now();
            for _ in 0..iterations {
                let d = Document::open(&args[2], ReaderOptions::default())?;
                let c = d.composition()?;
                black_box((&d, &c));
            }
            start.elapsed()
        }
        "extract" => {
            let d = Document::open(&args[2], ReaderOptions::default())?;
            let c = d.composition()?;
            let start = Instant::now();
            for _ in 0..iterations {
                for (i, e) in c
                    .essences
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.embedded_node.is_some())
                {
                    black_box(extract_to_file(
                        &d,
                        &c,
                        i,
                        ExtractFormat::Wave,
                        None,
                        &output.join(extraction_filename(e, ExtractFormat::Wave, None)),
                    )?);
                }
            }
            start.elapsed()
        }
        "profile" => {
            let d = Document::open(&args[2], ReaderOptions::default())?;
            let c = d.composition()?;
            eprintln!("profile PID {}", std::process::id());
            std::thread::sleep(std::time::Duration::from_secs(iterations as u64));
            black_box((&d, &c));
            return Ok(());
        }
        _ => return Err("unknown benchmark mode".into()),
    };
    // Emit integer nanoseconds so result formatting does not load floating-point
    // formatting code into the process whose peak memory is being measured.
    let nanoseconds = u64::try_from(elapsed.as_nanos() / iterations as u128)?;
    println!("{nanoseconds}");
    Ok(())
}
