use std::env;
use std::fs::File;

use cpm86_tools::cpmimg;


fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <diskimage> <user:filename.filetype> <destination>", args[0]);
        return Ok(());
    }

    let diskimage = &args[1];
    let source_file = &args[2];
    let destination_file = &args[3];

    // Parse argument
    let parts: Vec<&str> = source_file.split(|c| c == ':' || c == '.').collect();
    if parts.len() != 3 {
        eprintln!("Invalid format, expected user:filename.filetype");
        return Ok(());
    }

    let mut disk = File::open(diskimage)?;
    let catalog = cpmimg::read_catalog(&mut disk)?;

    let files: Vec<cpmimg::FileEntry> = cpmimg::merge_extents(catalog);
    println!("Filer på disken '{}':", diskimage);
    for entry in &files {
        println!("{:?} {}", entry, entry.file_size());
    }

    let mut out = File::create(destination_file)?;
    cpmimg::copy_out(files, source_file, &mut disk, &mut out)?;

    Ok(())
}
