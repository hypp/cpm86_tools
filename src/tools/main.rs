use clap::{Parser, Subcommand};
use anyhow::Result;

mod lib;
use crate::lib::cpmimg;

#[derive(Parser)]
#[clap(version, about = "A tool for COMPIS CP/M 86 raw floppy images.")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new empty floppy image: 512 bytes per sector, 8 sectors per track, 84 tracks.
    /// Ex: cpmtool create mycompis.img
    Create {
        /// Path to the new floppy image.
        #[clap(name = "IMAGE_FILE")]
        image_path: String,
        #[clap(name = "SIZE", value_enum, default_value_t = cpmimg::DiskSize::K640)]
        size: cpmimg::DiskSize,
    },
    /// Copy a file from local filesystem to the floppy image.
    /// Ex: cpmtool copyin mycompis.img myprog.bin 0:myprog.cmd
    Copyin {
        /// Path to the floppy image
        #[clap(name = "IMAGE_FILE")]
        image_path: String,
        /// Path to file in local filesystem
        #[clap(name = "SOURCE_FILE")]
        source_path: String,
        /// User:Name.Type of destination file in image
        #[clap(name = "CPM_FILE")]
        cpm_file_name: String,
    },
    /// Copy a file from the floppy image to the local filesystem.
    /// Ex: cpmtool copyout mycompis.img 0:myprog.cmd myprog.bin
    Copyout {
        /// Path to the floppy image
        #[clap(name = "IMAGE_FILE")]
        image_path: String,
        /// User:Name.Type of source file in image
        #[clap(name = "CPM_FILE")]
        cpm_file_name: String,
        /// Path to file in local filesystem
        #[clap(name = "TARGET_FILE")]
        output_path: String,
    },
    /// Delete a file from the floppy image.
    /// Ex: cpmtool delete mycompis.img 0:myprog.cmd
    Delete {
        /// Path to the floppy image
        #[clap(name = "IMAGE_FILE")]
        image_path: String,
        /// User:Name.Type of file in image
        #[clap(name = "CPM_FILE")]
        cpm_file_name: String,
    },
    /// List content of floppy image.
    /// Ex: cpmtool list mycompis.img
    List {
        /// Path to the floppy image
        #[clap(name = "IMAGE_FILE")]
        image_path: String,
    },
}


fn main() -> Result<()> {

    let cli = Cli::parse();

    match &cli.command {
        Commands::Create { image_path, size } => {
            cpmimg::create_image(image_path, size)?;
        }
        Commands::Copyin { image_path, source_path, cpm_file_name } => {
            cpmimg::copy_file_in(image_path, source_path, cpm_file_name)?;
        }
        Commands::Copyout { image_path, cpm_file_name, output_path } => {
            cpmimg::copy_file_out(image_path, cpm_file_name, output_path)?;
        }
        Commands::Delete { image_path, cpm_file_name } => {
            cpmimg::delete_file(image_path, cpm_file_name)?;
        }
        Commands::List { image_path } => {
            cpmimg::list_directory(image_path)?;
        }
    }

    Ok(())
}
