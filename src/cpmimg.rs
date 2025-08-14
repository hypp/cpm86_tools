
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write, Seek, SeekFrom};

const BLOCKSIZE: usize = 2048; // $800 byte
const DIRBLOCKS: usize = 2;
const MAXDIR: usize = 128;
const CATALOG_OFFSET: u64 = 0x2000; // katalogen börjar på $2000
const DATA_OFFSET: u64 = CATALOG_OFFSET+MAXDIR as u64*32;

#[derive(Debug)]
pub struct DirEntry {
    user_number: u8,       // UU
    filename: String,       // F1..F8
    filetype: String,       // T1..T3
    extent: u8,             // EX, låg byte
    s2: u8,                 // S2, hög byte
    s1: u8,                 // reserverad
    record_count: u8,       // RC
    allocation: Vec<u16>,   // AL-lista (blocknummer)
    readonly: bool,
    system: bool,
    entry_number: u16,
}

impl DirEntry {
    pub fn extent_size(&self) -> usize {
        if self.is_full_extent() {
            128 * 128 // full extent = 16 384 bytes
        } else {
            self.record_count as usize * 128
        }
    }

    pub fn is_full_extent(&self) -> bool {
        self.record_count >= 0x80
    }
}


#[derive(Debug)]
pub struct FileEntry {
    user_number: u8,
    filename: String,
    filetype: String,
    readonly: bool,
    system: bool,
    extents: Vec<DirEntry>,   // alla extenter för filen
}

impl FileEntry {
    pub fn file_size(&self) -> usize {
        self.extents.iter().map(|e| e.extent_size()).sum()
    }
}

pub fn read_catalog(disk: &mut File) -> std::io::Result<Vec<DirEntry>> {
    let mut catalog = Vec::new();

    disk.seek(SeekFrom::Start(CATALOG_OFFSET))?;
    let mut buffer = vec![0u8; BLOCKSIZE * DIRBLOCKS];
    disk.read_exact(&mut buffer)?;

    for i in 0..MAXDIR {
        let offset = i * 32; // katalogpost = 32 byte
        let entry = &buffer[offset..offset + 32];

        // User number = 0xE5 => tom post
        let user_number = entry[0];
        if user_number == 0xE5 {
            continue;
        }

        let filename = String::from_utf8_lossy(&entry[1..9]).trim().to_string();

        // Kontrollera toppbiten i T1 för readonly/system (valfritt att spara)
        let t1 = entry[9];
        let readonly = t1 & 0x80 != 0;
        let system = entry[10] & 0x80 != 0;

        let extent = entry[12]; // EX
        let s1 = entry[13];
        let s2 = entry[14];     // S2
        let record_count = entry[15];

        let entry_number = (32 * s2 as u16) + extent as u16;

        let filetype: String = entry[9..12]
            .iter()
            .map(|b| (b & 0x7F) as char) // maskar bort T1'-bit och T2'-bit
            .collect();

        let mut allocation = Vec::new();
        let al_bytes = &entry[16..32]; // 16 byte AL

        for chunk in al_bytes.chunks_exact(2) {
            let lo = chunk[0] as u16;
            let hi = chunk[1] as u16;
            let block = (hi << 8) | lo;
            if block != 0 {
                allocation.push(block);
            }
        }

        catalog.push(DirEntry {
            user_number,
            filename,
            filetype,
            extent,
            s2,
            s1,
            record_count,
            allocation,
            readonly,
            system,
            entry_number
        });
    }

    Ok(catalog)
}

pub fn merge_extents(entries: Vec<DirEntry>) -> Vec<FileEntry> {
    let mut files: HashMap<(u8, String, String), FileEntry> = HashMap::new();

    for entry in entries {
        let key = (entry.user_number, entry.filename.clone(), entry.filetype.clone());
        let file = files
            .entry(key.clone())
            .or_insert(FileEntry {
                user_number: entry.user_number,
                filename: entry.filename.clone(),
                filetype: entry.filetype.clone(),
                readonly: false,
                system: false,
                extents: Vec::new(),
            });
        file.readonly |= entry.extent & 0x80 != 0; // sätt readonly om någon extent har det
        file.system |= entry.extent & 0x80 != 0;   // sätt system om någon extent har det
        file.extents.push(entry);
    }

    let mut file_list: Vec<FileEntry> = files.into_values().collect();

    for item in &mut file_list {
        item.extents.sort_by_key(|extent| extent.entry_number);
    }

    file_list
}

pub fn copy_out(files: Vec<FileEntry>, source_file: &String, disk: &mut File, out: &mut File) -> std::io::Result<()> {
    let parts: Vec<&str> = source_file.split(|c| c == ':' || c == '.').collect();
    if parts.len() != 3 {
        eprintln!("Invalid format, expected user:filename.filetype");
        return Ok(());
    }

    let user: u8 = parts[0].parse().unwrap_or(0);
    let filename = parts[1].to_lowercase();
    let filetype = parts[2].to_lowercase();

    if let Some(file_entry) = files.iter().find(|f| {
        f.user_number == user &&
        f.filename.to_lowercase() == filename &&
        f.filetype.to_lowercase() == filetype
    }) {
        let total_size = file_entry.file_size();
        let mut written: usize = 0;

        for extent in &file_entry.extents {
            for &block in &extent.allocation {
                if block == 0 { continue; }
                let offset = DATA_OFFSET + block as u64 * BLOCKSIZE as u64; // blockstorlek
                disk.seek(SeekFrom::Start(offset))?;

                // Läs exakt antal bytes som behövs
                let remaining = total_size - written;
                let read_size = std::cmp::min(BLOCKSIZE, remaining);

                let mut buf = vec![0u8; read_size];
                disk.read_exact(&mut buf)?;
                out.write_all(&buf)?;

                written += read_size;
                if written >= total_size {
                    break;
                }
            }
            if written >= total_size {
                break;
            }
        }

        println!("File {}:{}.{}, copied to test.bin", user, file_entry.filename, file_entry.filetype);
    } else {
        eprintln!("File {} not found on disk", source_file);
    }

    Ok(())
}