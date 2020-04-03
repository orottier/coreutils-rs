//! `cat [OPTION]... [FILE]...`: concatenate FILE(s) to standard output
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - support options

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::process::exit;

use coreutils::print_help_and_exit;

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

enum Input {
    StdIn,
    File(String),
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut has_stdin = false; // only take stdin once

    let filenames: Vec<_> = args
        .flat_map(|s| match s.as_ref() {
            "-" if has_stdin => None,
            "-" => {
                has_stdin = true;
                Some(Input::StdIn)
            }
            s if s.starts_with('-') => print_help_and_exit(USAGE),
            _ => Some(Input::File(s)),
        })
        .collect();

    cat(filenames);
    exit(0)
}

/// `cat` implementation
fn cat(inputs: Vec<Input>) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    // get stdin lock, even if we don't need it
    // (we can't borrow on the fly)
    let stdin = io::stdin();

    inputs.into_iter().for_each(|input| {
        let mut read: Box<dyn BufRead> = match input {
            Input::StdIn => Box::new(stdin.lock()),
            Input::File(filename) => {
                match File::open(filename) {
                    Ok(file) => Box::new(BufReader::new(file)),
                    Err(_) => {
                        eprintln!("...");
                        return; // ignore this file
                    }
                }
            }
        };

        let result = io::copy(&mut read, &mut stdout);
        if result.is_err() {
            eprintln!("...");
        }
    });
}
