//! `tee [OPTION]... [FILE]...`: read from standard input and write to standard output and files
//!
//! The tee utility copies standard input to standard output, making a copy in zero or more files.
//! The output is unbuffered.
//!
//! Todo:
//!  - use `-i` to ignore the SIGINT signal

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::print_help_and_exit;

const USAGE: &str =
    "tee [OPTION]... [FILE]...\nread from standard input and write to standard output and files";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut filenames = vec![];

    let append = match args.next() {
        Some(s) if &s == "-a" => true,
        Some(s) if s.starts_with('-') => print_help_and_exit(USAGE),
        Some(filename) => (filenames.push(filename), false).1,
        None => false,
    };

    filenames.extend(args.into_iter());

    match tee(filenames, append) {
        Ok(_) => exit(0),
        Err(_) => exit(1),
    }
}

/// `tee` implementation
fn tee(filenames: Vec<String>, append: bool) -> io::Result<()> {
    // we do not need to wrap our files in bufwriters, since we will read stdin chunked
    let files = filenames
        .into_iter()
        .map(|filename| {
            OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .truncate(!append)
                .open(filename)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    loop {
        let buffer = stdin.fill_buf()?;
        if buffer.is_empty() {
            break;
        }

        // write to files first in case our stdout pipe breaks
        files
            .iter()
            .try_for_each(|mut write| write.write_all(buffer))?;

        stdout.write_all(buffer)?;

        let length = buffer.len();
        stdin.consume(length);
    }

    Ok(())
}
