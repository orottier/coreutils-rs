//! `sponge [-a] <file>`: soak up all input from stdin and write it to <file>
//!
//! sponge reads standard input and writes it out to the specified file. Unlike a shell redirect,
//! sponge soaks up all its input before opening the output file. This allows constricting
//! pipelines that read from and write to the same file.
//!
//! Todo:
//!  - use temp file instead of memory
//!  - then, update the output file atomically
//!  - preserve the permissions of the output file if it already exists

use std::convert::TryFrom;
use std::io::{self, Read, Write};
use std::process::exit;

use coreutils::{print_help_and_exit, Output, OutputArg};

const USAGE: &str = "sponge [-a] <file>: soak up all input from stdin and write it to <file>";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let output_arg = match args.next() {
        Some(s) if &s == "-a" => match args.next() {
            Some(filename) => OutputArg::File(filename, true),
            None => print_help_and_exit(USAGE),
        },
        Some(filename) => OutputArg::File(filename, false),
        None => OutputArg::Stdout,
    };

    if args.next().is_some() {
        print_help_and_exit(USAGE);
    }

    match sponge(output_arg) {
        Ok(_) => exit(0),
        Err(_) => exit(1),
    }
}

/// `sponge` implementation
fn sponge(output_arg: OutputArg<String>) -> io::Result<()> {
    let mut output = Output::try_from(&output_arg)?;

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut buffer = vec![];
    input.read_to_end(&mut buffer)?;

    output.write_all(&buffer)?;

    Ok(())
}
