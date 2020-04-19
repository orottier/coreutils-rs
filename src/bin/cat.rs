//! `cat [OPTION]... [FILE]...`: concatenate FILE(s) to standard output
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - support options

use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::io::{Input, InputArg};
use coreutils::util::print_help_and_exit;

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut has_stdin = false; // only take stdin once
    let mut line_numbers = false;

    let mut inputs: Vec<_> = args
        .flat_map(|s| match s.as_ref() {
            "-" if has_stdin => None,
            "-" => {
                has_stdin = true;
                Some(InputArg::Stdin)
            }
            "-n" => {
                line_numbers = true;
                None
            }
            s if s.starts_with('-') => print_help_and_exit(USAGE),
            _ => Some(InputArg::File(s)),
        })
        .collect();

    if inputs.is_empty() {
        inputs.push(InputArg::Stdin);
    }

    cat(&inputs, line_numbers);
    exit(0)
}

/// `cat` implementation
fn cat(input_args: &[InputArg<String>], line_numbers: bool) {
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
        .map(|input| input.into_bufread())
        .for_each(|mut input| {
            let result = if line_numbers {
                let mut n = 0u32;
                input
                    .lines()
                    .flat_map(|line| line.ok())
                    .try_for_each(|line| {
                        n += 1;
                        stdout.write_fmt(format_args!("{:>6} {}\n", n, line))
                    })
            } else {
                io::copy(&mut input, &mut stdout).map(|_| ())
            };

            if result.is_err() {
                eprintln!("...");
            }
        });
}
