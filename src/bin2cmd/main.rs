use clap::{Parser, Subcommand, ValueEnum};
use anyhow::Result;
use binrw::{BinRead, BinWrite, binrw};
use num_enum::TryFromPrimitive;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};

#[derive(Parser)]
#[clap(version, about = "Create a CP/M 86 .CMD-file from a .BIN-file")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new .CMD-file
    /// Ex: bin2cmd create myprog.cmd myprog.bin 
    Create {
        /// Path to the new .CMD-file.
        #[clap(name = "OUTPUT_FILE")]
        cmd_path: String,
        /// Path to the code file
        #[clap(name = "CODE_FILE")]
        code_path: String,
        // Optional load address
        #[clap(name = "LOAD_ADDRESS")]
        load_address: Option<u32>,
        // Path to optional data file
        #[clap(name = "DATA_FILE")]
        data_path: Option<String>,
        // Optional data load address
        #[clap(name = "DATA_LOAD_ADDRESS")]
        data_load_address: Option<u32>,
    },
}


//
// CMD header definition 
// http://www.s100computers.com/Software%20Folder/CPM86/CPM-86_System_Guide_Jun83.pdf
//

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum GType {
    Null = 0x0,
    Code = 0x1,
    Data = 0x2,
    Extra = 0x3,
    Stack = 0x4,
    AuxiliaryGroup1 = 0x5,
    AuxiliaryGroup2 = 0x6,
    AuxiliaryGroup3 = 0x7,
    AuxiliaryGroup4 = 0x8,
    SharedCodeGroup = 0x9,
    EsacepCode = 0xf,
}

impl GType {
    #[inline]
    pub fn from_low_nibble(n: u8) -> Self {
        GType::try_from(n & 0x0F).unwrap()
    }

    #[inline]
    pub fn to_low_nibble(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
pub struct GForm(pub u8);

impl GForm {
    #[inline] pub fn raw(self) -> u8 { self.0 }

    #[inline] pub fn g_type(self) -> GType {
        GType::from_low_nibble(self.0 & 0x0F)
    }

    #[inline] pub fn hi_nibble(self) -> u8 {
        self.0 >> 4
    }

    #[inline] pub fn with_type(self, t: GType) -> Self {
        GForm((self.0 & 0xF0) | t.to_low_nibble())
    }

    #[inline] pub fn with_hi(self, hi: u8) -> Self {
        GForm(((hi & 0x0F) << 4) | (self.0 & 0x0F))
    }

    #[inline] pub fn from_parts(t: GType, hi: u8) -> Self {
        GForm(((hi & 0x0F) << 4) | t.to_low_nibble())
    }
}

#[binrw]
#[brw(little)]
#[derive(Debug, Copy, Clone)]
pub struct GroupDescriptor {
    pub g_form: GForm,   
    pub g_length: u16,   // paragraphs (16-byte units)
    pub a_base: u16,     // base paragraph (0 = relocatable)
    pub g_min: u16,      // min paragraphs
    pub g_max: u16,      // max paragraphs
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone)]
pub struct CmdHeader {
    pub groups: [GroupDescriptor; 8], // 72 bytes
    pub padding: [u8; 56],           // padding to 128
}


fn create_image(cmd_path: &str, code_path: &str, load_address: &Option<u32>, data_path: &Option<String>, data_load_address: &Option<u32>) -> Result<()> {

    // The header, 8 GroupDescriptors and padding
    let mut header = CmdHeader {
    groups: [GroupDescriptor {
            g_form: GForm(0),
            g_length: 0,
            a_base: 0,
            g_min: 0,
            g_max: 0,
        }; 8],
        padding: [0u8; 56],
    };

    let mut out = File::create(cmd_path)?;

    let mut code_file= File::open(code_path)?;
    let mut code_data = Vec::new();
    code_file.read_to_end(&mut code_data)?;

    let code_len = code_data.len();
    let code_paragraphs = ((code_len + 15) / 16) as u16;
    while code_data.len() < code_paragraphs as usize*16 {
        code_data.push(0);
    }
    let code_a_base = (load_address.unwrap_or(0) / 16) as u16;

    header.groups[0] = GroupDescriptor {
        g_form: GForm(GType::Code as u8),
        g_length: code_paragraphs,
        a_base: code_a_base,
        g_min: code_paragraphs,
        g_max: code_paragraphs,
    };

    let mut data_data = Vec::new();
    if let Some(data_path) = data_path {
        let mut data_file = File::open(data_path)?;
        data_file.read_to_end(&mut data_data)?;

        let data_len = data_data.len();
        let data_paragraphs = ((data_len + 15) / 16) as u16;
        while data_data.len() < data_paragraphs as usize*16 {
            data_data.push(0);
        }
        let data_a_base = (data_load_address.unwrap_or(0) / 16) as u16;

        header.groups[1] = GroupDescriptor {
            g_form: GForm(GType::Data as u8),
            g_length: data_paragraphs,
            a_base: data_a_base,
            g_min: data_paragraphs,
            g_max: data_paragraphs,
        };
    }

    header.write(&mut out)?;
    out.write(&code_data)?;
    out.write(&data_data)?;

    Ok(())
}


fn main() -> Result<()> {

    let cli = Cli::parse();

    match &cli.command {
        Commands::Create { cmd_path, code_path , load_address, data_path, data_load_address} => {
            create_image(cmd_path, code_path, load_address, data_path, data_load_address)?;
        }
    }

    Ok(())
}
