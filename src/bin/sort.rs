//! `sort [OPTION]... [FILE]...`: sort lines of text files
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - more sort ordering
//!  - reverse
//!  - unique
//!  - parallel
//!  - batch size (limit memory usage)

use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::{print_help_and_exit, Input, InputArg};

const USAGE: &str = "sort [OPTION]... [FILE]... : sort lines of text files";

type Line = Box<[u8]>;

#[allow(dead_code)]
enum Ordering {
    Locale,
    Bytes,
    Numerical,
    CaseInsensitive,
    // ...
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut has_stdin = false; // only take stdin once
    let mut ordering = Ordering::Bytes;

    let mut inputs: Vec<_> = args
        .flat_map(|s| match s.as_ref() {
            "-" if has_stdin => None,
            "-" => {
                has_stdin = true;
                Some(InputArg::Stdin)
            }
            "-n" => {
                ordering = Ordering::Numerical;
                None
            }
            s if s.starts_with('-') => print_help_and_exit(USAGE),
            _ => Some(InputArg::File(s)),
        })
        .collect();

    if inputs.is_empty() {
        inputs.push(InputArg::Stdin);
    }

    sort(&inputs, ordering);
    exit(0)
}

/// `sort` handler
fn sort(input_args: &[InputArg<String>], _ordering: Ordering) {
    let line_iter = input_args
        .iter()
        .flat_map(|input_arg| {
            Input::try_from(input_arg)
                .map_err(|_| {
                    eprintln!("...");
                })
                .ok()
        })
        .flat_map(|input| input.into_bufread().split(b'\n'))
        .flat_map(|line| line.ok())
        .map(|line| line.into_boxed_slice()); // save bytes by dropping cap field

    let mut sorted = sort_vec(line_iter);
    // a bit slower:
    // let mut sorted = sort_tree(line_iter);

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    sorted
        .try_for_each(|line| {
            stdout
                .write_all(&line)
                .and_then(|_| stdout.write_all(&[b'\n']))
        })
        .unwrap();
}

fn sort_vec(line_iter: impl Iterator<Item = Line>) -> impl Iterator<Item = Line> {
    let mut lines: Vec<_> = line_iter.collect();
    lines.sort_unstable();
    lines.into_iter()
}

#[allow(dead_code)]
fn sort_tree(line_iter: impl Iterator<Item = Line>) -> impl Iterator<Item = Line> {
    let mut lines: BTreeMap<Line, usize> = BTreeMap::new();

    line_iter.for_each(|line| {
        lines.entry(line).and_modify(|v| *v += 1).or_insert(1);
    });

    lines
        .into_iter()
        .flat_map(|(k, v)| std::iter::repeat(k).take(v))
}
