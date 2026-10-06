//! Development-only direct extraction counterpart to tooling/extraction.c.
use libaaf::{
    Document, ReaderOptions,
    media::{self, ExtractFormat, FrameRange},
    timeline::EssenceFormat,
};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "usage: extraction <AAF> <original|wav|bwav> <directory> <offset> <length>".into(),
        );
    }
    let format = match args[2].as_str() {
        "wav" => ExtractFormat::Wave,
        "bwav" => ExtractFormat::BroadcastWave,
        "original" => ExtractFormat::Original,
        _ => return Err("invalid format".into()),
    };
    let offset: u64 = args[4].parse()?;
    let length: u64 = args[5].parse()?;
    let range = (offset != 0 || length != 0).then_some(FrameRange { offset, length });
    let d = Document::open(&args[1], ReaderOptions::default())?;
    let c = d.composition()?;
    for (index, (i, e)) in c
        .essences
        .iter()
        .enumerate()
        .filter(|(_, e)| e.embedded_node.is_some() && e.format != EssenceFormat::Video)
        .enumerate()
    {
        let mut named = e.clone();
        named.unique_name = format!("essence-{index}");
        let filename = media::extraction_filename(
            &named,
            format,
            range.as_ref().map(|_| named.unique_name.as_str()),
        );
        let result = media::extract_to_file(
            &d,
            &c,
            i,
            format,
            range,
            &Path::new(&args[3]).join(filename),
        );
        println!("{index}:{}", if result.is_ok() { "ok" } else { "error" });
    }
    Ok(())
}
