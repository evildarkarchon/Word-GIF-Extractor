//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! Everything the tool does — extraction and the terminal presentation of a run —
//! lives in the `word_image_extractor` library. This binary parses arguments, hands
//! them to the library with a real terminal, and returns the result: an intake
//! failure as the error, and otherwise the exit status the library chose.

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use word_image_extractor::{Args, TerminalOutput, run_cli};

fn main() -> Result<ExitCode> {
    run_cli(Args::parse(), TerminalOutput::stdio())
}
