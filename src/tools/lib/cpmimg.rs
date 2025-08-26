
use std::cmp::min;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use anyhow::Result;
use clap::{ValueEnum};

const NUM_SIDES: usize = 2;
// empirically tested with copydisk, and repeated usage of pip to fill a large disk image
// data equal to or above 0xa0000 is never touched
const NUM_TRACKS: usize = 80;
const NUM_SECTORS_PER_TRACK: usize = 8;
const NUM_BYTES_PER_SECTOR: usize = 512;
const TOTAL_DISKSIZE: usize = NUM_TRACKS*NUM_SECTORS_PER_TRACK*NUM_BYTES_PER_SECTOR*NUM_SIDES;

const BLOCKSIZE: usize = 16*128; // 16: 128 Byte Records / Block $800 bytes
const DIRBLOCKS: usize = 2;
const DIRENTRY_SIZE: usize = 32; // 128: 32 Byte  Directory Entries
const MAXDIR_ENTRIES: usize = 128; // 128: 32 Byte  Directory Entries
const CATALOG_OFFSET: u64 = 0x2000; // directory entries start at $2000
const DATA_OFFSET: u64 = CATALOG_OFFSET;

// TODO is this caclulation correct?
const MAX_NUM_BLOCKS: usize = (TOTAL_DISKSIZE-DATA_OFFSET as usize)/BLOCKSIZE;

// Data in the image is stored like this:
// $0000-$1000 side 0
// $1000-$2000 side 1
// $2000-$3000 side 0
// $3000-$4000 side 1
// ... and so on
// When copying to disk with pip
// side 0 is used first, increasing track number until track 80 is reached
// then side 1 is used, BUT backwards, decreasing track number
// The bios (or drive) hides this from CP/M-86 and it is not
// reflected in the directory structure, AL (allocations) keep increasing 

// If I run => stat dsk:
// 5,088: 128 Byte Record Capacity
//   636: Kilobyte Drive Capacity
//   128: 32 Byte  Directory Entries
//   128: Checked  Directory Entries
//   128: 128 Byte Records / Directory Entry
//    16: 128 Byte Records / Block
//    32: 128 Byte Records / Track
//     1: Reserved  Tracks
//


// https://forum.vcfed.org/index.php?threads/more-on-exidy-sorcerer-disk-images.68900/

// John Elliott
// 30 mars 2022 20:24:47
// till
// mkfs.cpm is likely not populating the boot sector. 
//If you examine it in a hex editor, the last byte (offset 01FFh) 
// is used by CP/M-86 to determine the capacity. This should be one of:
// 00h: 160k
// 01h: 320k
// 0Ch: 1200k (144FEAT)
// 10h: 360k (PCP/M-86)
// 11h: 720k (PCP/M-86)
// 40h: 360k (PCP/M-86)
// 48h: 720k (144FEAT)
// 90h: 1440k (144FEAT)

// 00h: 160k
// 01h: 320k
// 0Ch: 1200k (144FEAT)
// 10h: 360k (PCP/M-86)
// 11h: 720k (PCP/M-86)
// 40h: 360k (PCP/M-86)
// 48h: 720k (144FEAT)
// 90h: 1440k (144FEAT)

const DISKSIZE_OFFSET: usize = 0x1ff;

#[derive(Debug, Clone, ValueEnum)]
pub enum DiskSize {
    #[clap(name = "160K")]
    K160,
    #[clap(name = "320K")]
    K320,
    #[clap(name = "1200K")]
    K1200,
    #[clap(name = "360K")]
    K360,
    #[clap(name = "720K")]
    K720,
    #[clap(name = "360K2")]
    K360_2,
    #[clap(name = "720K2")]
    K720_2,
    #[clap(name = "1440K")]
    K1440,
    #[clap(name = "640K")]
    K640,
}

impl DiskSize {
    /// Returnerar ett hexvärde (kan vara typiskt för DPB, media descriptor byte etc.)
    fn hex_value(&self) -> u8 {
        match self {
            DiskSize::K160  => 0x00,
            DiskSize::K320  => 0x01,
            DiskSize::K1200  => 0x0c,
            DiskSize::K360  => 0x10,
            DiskSize::K720  => 0x11,
            DiskSize::K360_2  => 0x40,
            DiskSize::K720_2  => 0x48,
            DiskSize::K1440  => 0x90,
            DiskSize::K640 => 0xe5, // Don't know what to write here, 
        }
    }

    fn num_bytes(&self) -> usize {
        match self {
            DiskSize::K160  => 160*1024,
            DiskSize::K320  => 320*1024,
            DiskSize::K1200  => 1200*1024,
            DiskSize::K360  => 360*1024,
            DiskSize::K720  => 720*1024,
            DiskSize::K360_2  => 360*1024,
            DiskSize::K720_2  => 720*1024,
            DiskSize::K1440  => 1440*1024,
            DiskSize::K640 => 636*1024,
        }
    }
}

#[derive(Debug)]
struct DirEntry {
    directory_entry_idx: usize, // Index/Row in directory
    user_number: u8,       // UU
    filename: String,       // F1..F8
    filetype: String,       // T1..T3
    extent: u8,             // EX, low byte
    s2: u8,                 // S2, hi byte
    s1: u8,                 // reserved
    record_count: u8,       // RC
    allocation: Vec<u16>,   // AL-list (block numbers)
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

    pub fn write_to_file(&self, file: &mut File) -> Result<()> {
        let mut buf: Vec<u8> = Vec::new();

        buf.push(self.user_number);

        for c in self.filename.chars() {
            buf.push((c as u8) & 0x7F);
        }

        for c in self.filetype.chars() {
            buf.push((c as u8) & 0x7F);
        }

        // Extent
        buf.push(self.extent);
        buf.push(self.s1);
        buf.push(self.s2);
        buf.push(self.record_count);
        
        for al in self.allocation.iter() {
            let bytes = al.to_le_bytes();
            buf.push(bytes[0]);
            buf.push(bytes[1]);
        }

        if buf.len() > DIRENTRY_SIZE {
            anyhow::bail!("Directroy entry is to large for {}:{}.{} at {}",self.user_number, self.filename, self.filetype, self.directory_entry_idx);
        }

        while buf.len() < DIRENTRY_SIZE {
            buf.push(0);
        }

        let offset = CATALOG_OFFSET + self.directory_entry_idx as u64 * DIRENTRY_SIZE as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&buf)?;

        Ok(())
    }
}


#[derive(Debug)]
struct FileEntry {
    first_directory_entry_idx: usize,
    user_number: u8,
    filename: String,
    filetype: String,
    readonly: bool,
    system: bool,
    extents: Vec<DirEntry>,   // all extents for the file
}

impl FileEntry {
    pub fn file_size(&self) -> usize {
        self.extents.iter().map(|e| e.extent_size()).sum()
    }

    pub fn write_to_file(&self, file: &mut File) -> Result<()> {
        for entry in self.extents.iter() {
            entry.write_to_file(file)?;
        }
        Ok(())
    }    
}

fn read_catalog(disk: &mut File) -> Result<Vec<DirEntry>> {
    let mut catalog = Vec::new();

    disk.seek(SeekFrom::Start(CATALOG_OFFSET))?;
    let mut buffer = vec![0u8; BLOCKSIZE * DIRBLOCKS];
    disk.read_exact(&mut buffer)?;

    for idx in 0..MAXDIR_ENTRIES {
        let offset = idx * 32; // directory entry = 32 byte
        let entry = &buffer[offset..offset + 32];

        // User number = 0xE5 => empty directory entry
        let user_number = entry[0];
        if user_number == 0xE5 {
            continue;
        }

        let filename = String::from_utf8_lossy(&entry[1..9]).trim().to_string();

        // MSB is used as flag for readonly and system/hidden
        let t1 = entry[9];
        let readonly = t1 & 0x80 != 0;
        let system = entry[10] & 0x80 != 0;

        let extent = entry[12]; // EX
        let s1 = entry[13];
        let s2: u8 = entry[14];     // S2
        let record_count = entry[15];

        let entry_number = (32 * s2 as u16) + extent as u16;

        let filetype: String = entry[9..12]
            .iter()
            .map(|b| (b & 0x7F) as char) // Remove MSB
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
            directory_entry_idx: idx,
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
            entry_number,
        });
    }

    Ok(catalog)
}

fn merge_extents(entries: Vec<DirEntry>) -> Vec<FileEntry> {
    let mut files: HashMap<(u8, String, String), FileEntry> = HashMap::new();

    for entry in entries {
        let key = (entry.user_number, entry.filename.clone(), entry.filetype.clone());
        let file = files
            .entry(key.clone())
            .or_insert(FileEntry {
                first_directory_entry_idx: entry.directory_entry_idx,
                user_number: entry.user_number,
                filename: entry.filename.clone(),
                filetype: entry.filetype.clone(),
                readonly: false,
                system: false,
                extents: Vec::new(),
            });
        file.first_directory_entry_idx = min(entry.directory_entry_idx,file.first_directory_entry_idx);
        if entry.readonly {
            // Set readonly if any entry has readonly
            file.readonly = true;
        }
        if entry.system {
            // Set system if any entry has system
            file.system = true;
        }
        file.extents.push(entry);
    }

    let mut file_list: Vec<FileEntry> = files.into_values().collect();
    file_list.sort_by_key(|f| f.first_directory_entry_idx);

    for item in &mut file_list {
        item.extents.sort_by_key(|extent| extent.entry_number);
    }

    file_list
}

fn split_cpm_file_name(cpm_file_name: &str) -> Result<(u8, String, String)> {
    let parts: Vec<&str> = cpm_file_name.split(|c| c == ':' || c == '.').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid format, expected user:filename.filetype {}", cpm_file_name);
    }

    let user: u8 = parts[0].parse().unwrap_or(0);
    let filename = parts[1].to_uppercase();
    let filetype = parts[2].to_uppercase();

    if filename.len() > 8 || filetype.len() > 3 {
        anyhow::bail!("Filename too long {}", cpm_file_name);
    }

    Ok((user,filename,filetype))
}

fn get_file_entry<'a>(files: &'a Vec<FileEntry>, cpm_file_name: &str) -> Result<Option<&'a FileEntry>> {

    let (user,filename, filetype) = split_cpm_file_name(cpm_file_name)?;

    let file_entry = files.iter().find(|f| {
        f.user_number == user &&
        f.filename.to_uppercase() == filename &&
        f.filetype.to_uppercase() == filetype
    });

    Ok(file_entry)
}

fn allocation_to_offset(al: u16) -> usize {
    let even = (al & 0xfffe) as usize;
    let odd = (al & 1) as usize;
    if al < 0x9e {
        // allocations below 0x9e are on side 0
        // counting UP
        let offset = DATA_OFFSET as usize + even*BLOCKSIZE*NUM_SIDES+odd*BLOCKSIZE;
        offset
    } else {
        // allocations above 0x9d are on side 1
        // counting DOWN
        let offset = TOTAL_DISKSIZE - (even - 0x9d) * BLOCKSIZE*NUM_SIDES +odd*BLOCKSIZE;
        offset
    }
}

fn copy_out(files: Vec<FileEntry>, cpm_file_name: &str, disk: &mut File, out: &mut File) -> Result<()> {

    if let Some(file_entry) = get_file_entry(&files, cpm_file_name)? {
        let total_size = file_entry.file_size();
        let mut written: usize = 0;

        for extent in &file_entry.extents {
            for &block in &extent.allocation {
                if block == 0 { continue; }
                let offset =  allocation_to_offset(block) as u64;
                disk.seek(SeekFrom::Start(offset))?;

                let remaining = total_size - written;
                let read_size = min(BLOCKSIZE, remaining);

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

    } else {
        anyhow::bail!("File {} not found in image", cpm_file_name);
    }

    Ok(())
}

fn copy_in(files: Vec<FileEntry>, cpm_file_name: &str, disk: &mut File, input: &mut File) -> Result<()> {

    if let Some(_file_entry) = get_file_entry(&files, cpm_file_name)? {
        anyhow::bail!("File {} already exists in image", cpm_file_name);
    }

    let (user,filename, filetype) = split_cpm_file_name(cpm_file_name)?;

    // split the file in blocks
    let mut file_data = Vec::new();
    input.read_to_end(&mut file_data)?;
    // round up file length nearest 128
    let file_len = ((file_data.len() + 127) / 128) * 128;
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    while !file_data.is_empty() {
        let chunk_size = std::cmp::min(BLOCKSIZE, file_data.len());
        blocks.push(file_data.drain(..chunk_size).collect());
    }
    let blocks_needed = blocks.len();
    let entries_needed = (blocks_needed + 7) / 8; // 8 block per DirEntry

    // Make sure we have enough free entries
    let mut used_entries = vec![false; MAXDIR_ENTRIES];
    for f in &files {
        for e in &f.extents {
            used_entries[e.directory_entry_idx] = true;
        }
    }

    let mut free_entries = Vec::new();
    for (idx, used) in used_entries.iter().enumerate() {
        if !used {
            free_entries.push(idx);
        }
    }    

    if free_entries.len() < entries_needed {
        anyhow::bail!("Not enough free entries in directory. Free: {} Needed: {}", free_entries.len(), entries_needed);
    }

    // Make sure we have enough free blocks
    let mut used_blocks = vec![false; MAX_NUM_BLOCKS];
    // block 0 and 1 are reserved
    used_blocks[0] = true;
    used_blocks[1] = true;
    for f in &files {
        for e in &f.extents {
            for al in &e.allocation {
                let tmp = *al as usize;
                if tmp >= used_blocks.len() {
                    println!("Invalid block number {} for file {}", al, f.filename);
                    continue;
                } 
                used_blocks[tmp as usize] = true;
            }
        }
    }

    let mut free_blocks = Vec::new();
    for (idx, used) in used_blocks.iter().enumerate() {
        if !used {
            free_blocks.push(idx as u16);
        }
    }    

    // Now create DirEntry and all FileEntry:s
    let mut file_entries: Vec<DirEntry> = Vec::new();
    let mut free_block_iter = free_blocks.into_iter();
    let mut blocks_left = blocks_needed;
    let mut file_len_left = file_len;
    for i in 0..entries_needed {
        let directory_entry_idx = free_entries[i];
        let mut al_list: Vec<u16> = Vec::new();
        for _ in 0..min(8, blocks_left) {
            if let Some(block) = free_block_iter.next() {
                al_list.push(block);
                blocks_left -= 1;
            }
        }

        let mut record_count: u8 = 0x80;
        if al_list.len() < 8 {
            // only happens on last iteration
            record_count = (file_len_left / 128) as u8;
        } else {
            file_len_left -= 8*BLOCKSIZE
        }

        let entry = DirEntry {
            directory_entry_idx,
            user_number: user,
            filename: filename.clone(),
            filetype: filetype.clone(),
            extent: (i & 0x1f) as u8,
            s2: ((i >> 5) & 0xff) as u8,
            s1: 0,
            record_count,
            allocation: al_list,
            readonly: false,
            system: false,
            entry_number: i as u16,
        };

        file_entries.push(entry);
    }

    let entry = FileEntry {
        first_directory_entry_idx: file_entries[0].directory_entry_idx,
        user_number: file_entries[0].user_number,
        filename,
        filetype,
        readonly: false,
        system: false,
        extents: file_entries
    };

    entry.write_to_file(disk)?;

    let mut iter = blocks.into_iter(); 
    for e in &entry.extents {
        for al in &e.allocation {
            let offset = allocation_to_offset(*al) as u64;
            let block = iter.next().unwrap();
            disk.seek(SeekFrom::Start(offset))?;
            disk.write_all(&block)?;
        }
    }    

    Ok(())
}


pub fn create_image(image_path: &str, size: &DiskSize) -> Result<()> {
    let mut out = File::create(image_path)?;
    let mut buf = [0u8; NUM_BYTES_PER_SECTOR];

    let num_tracks = size.num_bytes() / NUM_BYTES_PER_SECTOR / NUM_SECTORS_PER_TRACK;        

    for i in 0..buf.len() {
        // e5 is used as empty directory entry
        buf[i] = 0xe5;
    }

    for _ in 0..num_tracks {
        for _ in 0..NUM_SECTORS_PER_TRACK {
            out.write(&buf)?;
        }
    }

    // Write the magic byte to the disk type offset
    out.seek(SeekFrom::Start(DISKSIZE_OFFSET as u64))?;
    out.write(&[size.hex_value()])?;

    Ok(())
}

pub fn list_directory(image_path: &str) -> Result<()> {
    let mut disk = File::open(image_path)?;
    let catalog = read_catalog(&mut disk)?;
    let files: Vec<FileEntry> = merge_extents(catalog);

    println!("Files in image '{}':", image_path);
    println!("UID Name     Ext     Size Readonly System");
    println!("------------------------------------------");
    for entry in &files {
        println!("{:>3} {:>8} {:>3} {:>8} {:>8} {:>6}", entry.user_number, entry.filename, entry.filetype, entry.file_size(), entry.readonly, entry.system);
    }

    Ok(())
}

pub fn copy_file_in(image_path: &str, source_path: &str, cpm_file_name: &str) -> Result<()> {
    let mut disk = OpenOptions::new()
                .read(true)
                .write(true)
                .open(image_path)?;
    let catalog = read_catalog(&mut disk)?;
    let files: Vec<FileEntry> = merge_extents(catalog);

    let mut input = File::open(source_path)?;
    copy_in(files, cpm_file_name, &mut disk, &mut input)?;
    
    Ok(())
}

pub fn copy_file_out(image_path: &str, cpm_file_name: &str, output_path: &str) -> Result<()> {
    let mut disk = File::open(image_path)?;
    let catalog = read_catalog(&mut disk)?;
    let files: Vec<FileEntry> = merge_extents(catalog);

    let mut out = File::create(output_path)?;
    copy_out(files, cpm_file_name, &mut disk, &mut out)?;

    Ok(())
}


