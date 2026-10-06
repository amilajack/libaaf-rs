//! Development-only original-property/reference inspection oracle.
use libaaf::{Document, ReaderOptions};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: inspection <AAF>")?;
    let d = Document::open(&path, ReaderOptions::default())?;
    for o in d.objects() {
        print!("O\t{}\t", o.node);
        for b in o.class.0 {
            print!("{b:02x}");
        }
        println!();
        for p in o.properties() {
            print!("P\t{}\t{:04x}\t{:04x}\t", o.node, p.id, p.stored_form);
            for b in o.raw(p.id).unwrap_or_default() {
                print!("{b:02x}");
            }
            print!("\t");
            for (i, &id) in p.references.iter().enumerate() {
                if i != 0 {
                    print!(",");
                }
                print!("{}", d.object(id).node);
            }
            println!();
        }
    }
    Ok(())
}
