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
use std::num::NonZeroU32;
use std::process::exit;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use coreutils::chunks::ChunkedReader;
use coreutils::executor::{Job, ThreadPool};
use coreutils::io::{Input, InputArg};
use coreutils::util::print_help_and_exit;

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

struct WcJob(Arc<AtomicU64>, Arc<AtomicU64>, Arc<AtomicU64>, Vec<u8>);

impl Job for WcJob {
    fn run(self) {
        let WcJob(chars_total, words_total, lines_total, chunk) = self;

        let mut n_chars = 0u64;
        let mut n_words = 0u64;
        let mut n_lines = 0u64;
        let mut prev_was_whitespace = true;

        std::str::from_utf8(&chunk).unwrap().chars().for_each(|c| {
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

        chars_total.fetch_add(n_chars, Ordering::Relaxed);
        words_total.fetch_add(n_words, Ordering::Relaxed);
        lines_total.fetch_add(n_lines, Ordering::Relaxed);
    }
}

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
fn wc(
    input_args: &[InputArg<String>],
    _bytes: bool,
    _chars: bool,
    _words: bool,
    _lines: bool,
    _max_line_length: bool,
) {
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

    let mut executor = ThreadPool::new(NonZeroU32::new(num_cpus::get() as u32).unwrap());

    let chars_total = Arc::new(AtomicU64::new(0));
    let words_total = Arc::new(AtomicU64::new(0));
    let lines_total = Arc::new(AtomicU64::new(0));

    let reader = ChunkedReader::new(inputs, b'\n', 1 << 25);
    reader.for_each(|chunk| {
        let c = chars_total.clone();
        let w = words_total.clone();
        let l = lines_total.clone();
        let job = WcJob(c, w, l, chunk.unwrap());
        executor.submit(job);
    });

    executor.finish();

    println!(
        "{} {} {}",
        chars_total.load(Ordering::Relaxed),
        words_total.load(Ordering::Relaxed),
        lines_total.load(Ordering::Relaxed)
    );
}
