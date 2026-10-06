use libaaf::{
    Document, ReaderOptions,
    inspection::{self, DisplayOptions, PositionFormat},
    media::{self, ExtractFormat},
    timeline::EssenceFormat,
};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
    path::PathBuf,
};
const COMMANDS: &[&str] = &[
    "cfb-header",
    "cfb-fat",
    "cfb-minifat",
    "cfb-difat",
    "cfb-nodes",
    "cfb-node",
    "aaf-summary",
    "aaf-essences",
    "aaf-clips",
    "aaf-classes",
    "aaf-meta",
    "aaf-properties",
    "trace",
    "extract-essences",
    "extract-clips",
    "extract-mobid",
];
const VALUES: &[&str] = &[
    "cfb-node",
    "dump-class",
    "dump-class-raw",
    "extract-path",
    "extract-format",
    "media-location",
    "samplerate",
    "pos-format",
    "log-file",
    "verb",
];
const SWITCHES: &[&str] = &[
    "dump-meta",
    "dump-tagged-value",
    "pt-true-fades",
    "pt-remove-sae",
    "show-automation",
    "show-metadata",
    "relative-path",
    "no-color",
];
fn help() {
    eprint!("{}", include_str!("help.txt"));
}
fn fail(s: &str) -> String {
    format!("Command line error: {s}\nTry 'aaftool --help' for more informations.")
}
fn main() {
    println!(
        "\naaftool Copyright (c) 2017-2026 Adrien Gesta-Fline\nlibaaf-rs {}\n",
        env!("CARGO_PKG_VERSION")
    );
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(255);
    }
}
fn run() -> Result<(), String> {
    let mut flags = HashSet::new();
    let mut values = HashMap::new();
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    let mut positional = false;
    while let Some(arg) = args.next() {
        if positional {
            files.push(arg);
            continue;
        }
        if arg == "--" {
            positional = true;
            continue;
        }
        if arg == "--help" || arg == "-h" {
            help();
            return Ok(());
        }
        if let Some(arg) = arg.strip_prefix("--") {
            let (name, inline) = arg
                .split_once('=')
                .map(|(a, b)| (a, Some(b.to_owned())))
                .unwrap_or((arg, None));
            if VALUES.contains(&name) {
                let v = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| fail(&format!("option '--{name}' requires an argument")))?;
                values.insert(name.to_owned(), v);
                flags.insert(name.to_owned());
            } else if COMMANDS.contains(&name) || SWITCHES.contains(&name) {
                if inline.is_some() {
                    return Err(fail(&format!(
                        "option '--{name}' doesn't allow an argument"
                    )));
                }
                flags.insert(name.to_owned());
            } else {
                return Err(fail(&format!("unrecognized option '--{name}'")));
            }
        } else if arg.starts_with('-') {
            return Err(fail(&format!("unrecognized option '{arg}'")));
        } else {
            files.push(arg);
        }
    }
    let has = |s: &str| flags.contains(s);
    let val = |s: &str| values.get(s).map(String::as_str);
    let file = files.last().ok_or_else(|| fail("missing AAF file"))?;
    if !COMMANDS.iter().any(|s| has(s)) {
        return Err(fail("at least one analysis or extract option is required."));
    }
    for name in ["extract-clips", "extract-essences"] {
        if has(name) && val("extract-path").is_none() {
            return Err(fail(&format!(
                "--{name} requires --extract-path to be set."
            )));
        }
    }
    if has("extract-clips") && has("extract-essences") {
        return Err(fail(
            "can't use --extract-clips and --extract-essences at the same time.",
        ));
    }
    let format = match val("extract-format") {
        None => ExtractFormat::Original,
        Some("wav") => ExtractFormat::Wave,
        Some("bwav") => ExtractFormat::BroadcastWave,
        _ => return Err(fail("wrong --extract-format <value>")),
    };
    let position = match val("pos-format") {
        None | Some("tc") => PositionFormat::Timecode,
        Some("hms") => PositionFormat::Hms,
        Some("raw") => PositionFormat::Raw,
        Some("samples") => PositionFormat::Samples,
        _ => return Err(fail("wrong --pos-format <value>")),
    };
    let verb = val("verb")
        .unwrap_or("2")
        .parse::<u8>()
        .map_err(|_| fail("wrong --verb <value>"))?;
    if verb > 3 {
        return Err(fail("wrong --verb <value>"));
    }
    let sample_rate = val("samplerate")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0);
    let options = ReaderOptions {
        media_location: val("media-location").map(PathBuf::from),
        recover_protools_fades: has("pt-true-fades"),
        remove_sample_accurate_edits: has("pt-remove-sae"),
        mobid_filenames: has("extract-mobid"),
    };
    let doc = Document::open(file, options).map_err(|_| format!("Failed to open {file}"))?;
    let c = doc
        .composition()
        .map_err(|_| format!("Failed to open {file}"))?;
    let display = DisplayOptions {
        position,
        sample_rate,
        automation: has("show-automation"),
        metadata: has("show-metadata"),
        relative_paths: has("relative-path"),
    };
    let mut output: Box<dyn Write> = if let Some(path) = val("log-file") {
        Box::new(
            std::fs::File::create(path)
                .map_err(|_| format!("Failed to open logfile for writting : {path}"))?,
        )
    } else {
        Box::new(io::stdout().lock())
    };
    let mut s = String::from("\n");
    if let Some(path) = val("cfb-node") {
        if let Some(id) = doc.cfb.node_by_path(path) {
            s += &inspection::node(&doc.cfb, id);
            if doc.cfb.nodes[id as usize].kind == 2 {
                let stream = doc.cfb.stream(id).map_err(|e| e.to_string())?;
                if doc.cfb.nodes[id as usize].name.starts_with("properties") {
                    s += &inspection::raw_properties(&doc, &stream.to_vec(), " ")
                        .map_err(|e| e.to_string())?;
                } else {
                    s += &inspection::hex(&stream.to_vec(), " ");
                }
            }
        } else {
            eprintln!("Could not find node at \"{path}\"");
        }
        s.push_str("\n\n");
    }
    if has("cfb-header") {
        s += &inspection::cfb_header(&doc.cfb);
    }
    for (flag, kind) in [
        ("cfb-fat", "FAT"),
        ("cfb-minifat", "MiniFAT"),
        ("cfb-difat", "DiFAT"),
    ] {
        if has(flag) {
            s += &inspection::allocation(&doc.cfb, kind);
        }
    }
    if has("cfb-nodes") {
        s += &inspection::nodes(&doc.cfb);
    }
    if has("aaf-summary") {
        s += &inspection::summary(&doc, &c, &display);
    }
    if has("aaf-essences") {
        s += &inspection::essences(&doc, &c, &display);
    }
    if has("aaf-clips") {
        s += &inspection::clips(&c, &display);
    }
    if has("aaf-classes") {
        s += &inspection::classes(&doc);
    }
    if has("aaf-meta") {
        s += &inspection::dictionary(&doc);
    }
    if has("aaf-properties")
        || has("trace")
        || has("dump-meta")
        || has("dump-tagged-value")
        || has("dump-class")
        || has("dump-class-raw")
    {
        for o in &doc.objects {
            let name = doc.class_name(o.class);
            let selected = has("aaf-properties")
                || val("dump-class") == Some(name.as_str())
                || val("dump-class") == Some(name.trim_start_matches("AAFClassID_"));
            if has("trace") {
                s += &format!(" {} :: {}\n", doc.cfb.path(o.node), name);
            }
            if selected {
                s += &format!(" {} :: {}\n", doc.cfb.path(o.node), name);
                s += &inspection::object_properties(&doc, o.id);
            }
            if val("dump-class-raw")
                .is_some_and(|v| v == name || v == name.trim_start_matches("AAFClassID_"))
            {
                let node = doc.cfb.child(o.node, "properties").unwrap();
                s += &inspection::raw_properties(
                    &doc,
                    &doc.cfb.stream(node).map_err(|e| e.to_string())?.to_vec(),
                    " ",
                )
                .map_err(|e| e.to_string())?;
            }
            if has("dump-meta") {
                for p in &o.properties {
                    if doc
                        .classes
                        .iter()
                        .flat_map(|c| &c.properties)
                        .any(|d| d.custom && d.id == p.id)
                    {
                        s += &format!(" {name}::{}\n", doc.property_name(p.id));
                        s += &inspection::hex(o.raw(p.id).unwrap(), " ");
                    }
                }
            }
            if has("dump-tagged-value") {
                for p in &o.properties {
                    for (n, v) in doc.tagged(o.id, p.id) {
                        s += &format!(" Tagged > Name: {n}      Value: {v}\n");
                    }
                }
            }
        }
    }
    if has("extract-essences") || has("extract-clips") {
        let path = PathBuf::from(val("extract-path").unwrap());
        let mut found = false;
        if has("extract-essences") {
            for (i, e) in c
                .essences
                .iter()
                .enumerate()
                .filter(|(_, e)| e.format != EssenceFormat::Video && e.embedded_node.is_some())
            {
                found = true;
                let target = path.join(media::extraction_filename(e, format, None));
                match media::extract_to_file(&doc, &c, i, format, None, &target) {
                    Ok(_) => {
                        s += &format!(
                            "[success] Audio essence file extracted to \"{}\"\n",
                            target.display()
                        )
                    }
                    Err(_) => {
                        s += &format!(
                            "[ error ] Audio essence file extraction failed : \"{}\"\n",
                            e.unique_name
                        )
                    }
                }
            }
            if !found {
                s += "[ error ] File has no embedded essence to extract.\n";
            }
        }
        if has("extract-clips") {
            for t in c.tracks.iter().filter(|t| !t.video) {
                for (i, clip) in t.items.iter().filter_map(|i| i.clip()).enumerate() {
                    if let Some(source) = clip.sources.first() {
                        let e = &c.essences[source.essence];
                        let name = format!("{}_{}_{}", t.number, i + 1, e.unique_name);
                        let target = path.join(media::extraction_filename(e, format, Some(&name)));
                        let result = media::clip_range(clip, t, e).and_then(|r| {
                            media::extract_to_file(
                                &doc,
                                &c,
                                source.essence,
                                format,
                                Some(r),
                                &target,
                            )
                        });
                        match result {
                            Ok(_) => {
                                s += &format!(
                                    "[success] Audio clip file extracted to \"{}\"\n",
                                    target.display()
                                )
                            }
                            Err(err) => {
                                if verb > 0 {
                                    s += &format!(
                                        "[ error ] Audio clip file extraction failed : \"{name}\": {err}\n"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    output.write_all(s.as_bytes()).map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())?;
    Ok(())
}
