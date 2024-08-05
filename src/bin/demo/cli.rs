use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to the Amberstar data files
    #[arg(short, long, default_value = "./data")]
    pub data: PathBuf,

    /// Output path
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Clone)]
pub enum Command {
    /// Extract all data files
    Extract{ filename: PathBuf },
    /// Extract the dictionary
    Words,
    /// Extract all map strings
    Strings,
    /// Plays the song with the given song number
    Song { song : Option<usize> },
    /// Graphics demo (mainly intended for debugging and exploration)
    GfxDemo,
    /// Map viewer and 3D map walking demo
    MapViewer,
}
