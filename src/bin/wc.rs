//! `wc`: print newline, word, and byte counts for each file
//!
//! Print  newline, word, and byte counts for each FILE, and a total line if more than one FILE is
//! specified.  A word is a non-zero-length sequence of characters delimited by white space.
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - support options

use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::io::{Input, InputArg};
use coreutils::chunks::ChunkedReader;
use coreutils::util::print_help_and_exit;

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut has_stdin = false; // only take stdin once

    let mut bytes = false;
    let mut chars = false;
    let mut words = false;
    let mut lines = false;
    let mut max_line_length = false;

    let mut inputs: Vec<_> = args
        .flat_map(|s| match s.as_ref() {
            "-" if has_stdin => None,
            "-" => {
                has_stdin = true;
                Some(InputArg::Stdin)
            }
            "-b" => {
                bytes = true;
                None
            }
            "-c" => {
                chars = true;
                None
            }
            "-w" => {
                words = true;
                None
            }
            "-l" => {
                lines = true;
                None
            }
            "-L" => {
                max_line_length = true;
                None
            }
            s if s.starts_with('-') => print_help_and_exit(USAGE),
            _ => Some(InputArg::File(s)),
        })
        .collect();

    if inputs.is_empty() {
        inputs.push(InputArg::Stdin);
    }

    if !bytes && !chars && !words && !lines && !max_line_length {
        chars = true;
        words = true;
        lines = true;
    }

    wc(&inputs, bytes, chars, words, lines, max_line_length);
    exit(0)
}

/// `cat` implementation
fn wc(input_args: &[InputArg<String>], bytes: bool, chars: bool, words: bool, lines: bool, max_line_length: bool) {

    let inputs = input_args
        .iter()
        .flat_map(|input_arg| {
            Input::try_from(input_arg)
                .map_err(|_| {
                    eprintln!("...");
                })
                .ok()
        })
        .map(|input| input.into_read())
        .collect();

    let mut n_chars = 0u64;
    let mut n_words = 0u64;
    let mut n_lines = 0u64;
    let mut prev_was_whitespace = true;

    let mut reader = ChunkedReader::new(inputs, b'\n', 1 << 25);
    reader.for_each(|chunk| {
        std::str::from_utf8(&chunk.unwrap()).unwrap().chars().for_each(|c| {
            n_chars += 1;
            if c.is_whitespace() {
                if !prev_was_whitespace {
                    n_words += 1;
                    prev_was_whitespace = true;
                }

                if c == '\n' {
                    n_lines += 1;
                }
            } else {
                prev_was_whitespace = false;
            }
        });

        // count the newline at the end of this chunk
        n_chars += 1;
        n_lines += 1;

        if !prev_was_whitespace {
            n_words += 1;
        }
    });

    println!("{} {} {}", n_chars, n_words, n_lines);
}
