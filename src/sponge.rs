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

use std::fs::OpenOptions;
use std::io::{self, Read, Write};

const USAGE: &str = "sponge [-a] <file>: soak up all input from stdin and write it to <file>";

enum Output {
    StdOut,
    File(String),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args();
    args.next(); // bin name

    let (append, output) = match args.next() {
        Some(s) if &s == "-a" => match args.next() {
            Some(filename) => (true, Output::File(filename)),
            None => panic!(USAGE),
        },
        Some(filename) => (false, Output::File(filename)),
        None => (false, Output::StdOut),
    };

    if args.next().is_some() {
        panic!(USAGE);
    }

    let mut write: Box<dyn Write> = match output {
        Output::StdOut => Box::new(io::stdout()),
        Output::File(filename) => {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .truncate(!append)
                .open(filename)?;
            Box::new(file)
        }
    };

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut buffer = vec![];
    input.read_to_end(&mut buffer)?;

    write.write_all(&buffer)?;

    Ok(())
}
