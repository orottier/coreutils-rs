//! `cat [OPTION]... [FILE]...`: concatenate FILE(s) to standard output
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - support options

use std::convert::TryFrom;
use std::io;
use std::process::exit;

use coreutils::{print_help_and_exit, Input, InputArg};

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut has_stdin = false; // only take stdin once

    let inputs: Vec<_> = args
        .flat_map(|s| match s.as_ref() {
            "-" if has_stdin => None,
            "-" => {
                has_stdin = true;
                Some(InputArg::Stdin)
            }
            s if s.starts_with('-') => print_help_and_exit(USAGE),
            _ => Some(InputArg::File(s)),
        })
        .collect();

    cat(&inputs);
    exit(0)
}

/// `cat` implementation
fn cat(input_args: &[InputArg]) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    input_args
        .iter()
        .flat_map(|input_arg| {
            Input::try_from(input_arg)
                .map_err(|_| {
                    eprintln!("...");
                })
                .ok()
        })
        .for_each(|input| {
            let result = io::copy(&mut input.as_bufread(), &mut stdout);

            if result.is_err() {
                eprintln!("...");
            }
        });
}
