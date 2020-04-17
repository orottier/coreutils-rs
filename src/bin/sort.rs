//! `sort [OPTION]... [FILE]...`: sort lines of text files
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - more sort ordering
//!  - reverse
//!  - unique
//!  - parallel

use std::collections::BinaryHeap;
use std::convert::TryFrom;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::num::NonZeroUsize;
use std::process::exit;
use std::time::Instant;

use std::io::Split;
use std::vec::IntoIter;

use coreutils::{Input, InputArg};

use clap::{App, Arg};
use tempfile::tempfile;

const USAGE: &str = "Sort lines of text files";

type Line = Box<[u8]>;

#[allow(dead_code)]
enum SortOrder {
    Locale,
    Bytes,
    Numerical,
    CaseInsensitive,
    // ...
}

#[derive(Debug)]
enum LineIterator {
    File(Split<BufReader<File>>),
    Vec(IntoIter<Line>),
}
impl Iterator for LineIterator {
    type Item = Line;

    fn next(&mut self) -> Option<Line> {
        match self {
            LineIterator::File(iter) => iter.next().map(|l| l.expect("I/O").into_boxed_slice()),
            LineIterator::Vec(iter) => iter.next(),
        }
    }
}

#[derive(Debug)]
struct SortedChunk {
    source: LineIterator,
    head: Option<Line>,
}

impl Iterator for SortedChunk {
    type Item = Line;

    fn next(&mut self) -> Option<Line> {
        let next = self.source.next();
        std::mem::replace(&mut self.head, next)
    }
}

impl SortedChunk {
    pub fn new(lines: Vec<Line>) -> Self {
        let iter = lines.into_iter();

        let mut me = Self {
            source: LineIterator::Vec(iter),
            head: None,
        };

        me.next();

        me
    }

    pub fn flush_to_disk(&mut self) {
        // no-op if already file
        if let LineIterator::File(_) = self.source {
            return;
        }

        let mut tempfile = tempfile().unwrap();
        {
            let mut write = BufWriter::new(&mut tempfile);
            self.source
                .try_for_each(|line| {
                    write
                        .write_all(&line)
                        .and_then(|_| write.write_all(&[b'\n']))
                })
                .unwrap();
        }
        tempfile.seek(SeekFrom::Start(0)).unwrap();

        let iter = BufReader::new(tempfile).split(b'\n');

        *self = Self {
            source: LineIterator::File(iter),
            head: None,
        };

        self.next();
    }

    fn peek(&self) -> Option<&Line> {
        self.head.as_ref()
    }

    pub fn drain<W: Write>(&mut self, stdout: &mut W) {
        self.for_each(|line| {
            stdout
                .write_all(&line)
                .and_then(|_| stdout.write_all(&[b'\n']))
                .unwrap();
        });
    }

    pub fn drain_until<W: Write>(&mut self, stdout: &mut W, limit: &Line) -> bool {
        while let Some(head) = self.next() {
            stdout
                .write_all(&head)
                .and_then(|_| stdout.write_all(&[b'\n']))
                .unwrap();

            if self.peek().is_some() && self.peek().unwrap() > limit {
                return false;
            }
        }

        true
    }
}

impl PartialEq for SortedChunk {
    fn eq(&self, other: &Self) -> bool {
        self.peek() == other.peek()
    }
}
impl Eq for SortedChunk {}

use std::cmp::Ordering;

impl Ord for SortedChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.peek(), other.peek()) {
            (Some(l1), Some(l2)) => l2.cmp(&l1),
            _ => Ordering::Equal, // these cases will not occur
        }
    }
}
impl PartialOrd for SortedChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let matches = App::new("sort")
        .about(USAGE)
        .arg(
            Arg::with_name("numeric-sort")
                .long("numeric-sort")
                .short("n")
                .help("compare according to string numerical value"),
        )
        .arg(
            Arg::with_name("batch-size")
                .long("batch-size")
                .value_name("NMERGE")
                .help("merge at most NMERGE inputs at once; for more use temp files")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("buffer-size")
                .long("buffer-size")
                .value_name("SIZE")
                .short("S")
                .help("use SIZE for main memory buffer")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("FILE")
                .help("sort FILE(s), use - for stdin")
                .multiple(true)
                .index(1),
        )
        .get_matches();

    let mut inputs = matches
        .values_of("FILE")
        .map(|values| {
            values
                .map(|v| {
                    if v == "-" {
                        InputArg::Stdin
                    } else {
                        InputArg::File(v)
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| vec![]);
    if inputs.is_empty() {
        inputs.push(InputArg::Stdin);
    }

    let sort_order = if matches.is_present("numerical-sort") {
        SortOrder::Numerical
    } else {
        SortOrder::Bytes
    };

    let batch_size = matches
        .value_of("batch-size")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(NonZeroUsize::new);

    let buffer_size = matches
        .value_of("buffer-size")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(NonZeroUsize::new);

    sort(&inputs[..], sort_order, batch_size, buffer_size);
    exit(0)
}

/// `sort` handler
fn sort(
    input_args: &[InputArg<&str>],
    _sort_order: SortOrder,
    batch_size: Option<NonZeroUsize>,
    buffer_size: Option<NonZeroUsize>,
) {
    let mut line_iter = input_args
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

    let batch_size = batch_size.map(|b| b.get()).unwrap_or(usize::max_value());
    let buffer_size = buffer_size.map(|b| b.get()).unwrap_or(usize::max_value());
    eprintln!(
        "batch_size {}k, buffer_size {}M",
        batch_size >> 10,
        buffer_size >> 20
    );

    let mut exhausted = false;
    let mut bytes = 0;
    let mut chunks = vec![];
    let now = Instant::now();

    while !exhausted {
        let mut lines = vec![];
        let mut count = 0;

        while count < batch_size {
            if let Some(line) = line_iter.next() {
                bytes += line.len();
                lines.push(line);
            } else {
                exhausted = true;
                break;
            }
            count += 1;
        }
        eprintln!(
            "{:>5} - collected {} lines",
            Instant::now().duration_since(now).as_millis(),
            count
        );

        lines.sort_unstable();
        eprintln!(
            "{:>5} - sorted",
            Instant::now().duration_since(now).as_millis()
        );

        let chunk = SortedChunk::new(lines);

        if !exhausted && bytes > buffer_size {
            chunks
                .iter_mut()
                .for_each(|c: &mut SortedChunk| c.flush_to_disk());
            eprintln!(
                "{:>5} - flushed all to disk",
                Instant::now().duration_since(now).as_millis()
            );
            bytes = 0;
        }

        chunks.push(chunk);
    }

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let mut chunks: BinaryHeap<_> = chunks.into_iter().collect();

    while let Some(mut chunk) = chunks.pop() {
        let exhausted = match chunks.peek().and_then(|c| c.peek()) {
            Some(limit) => chunk.drain_until(&mut stdout, limit),
            None => {
                chunk.drain(&mut stdout);
                true
            }
        };
        if !exhausted {
            chunks.push(chunk);
        }
    }

    eprintln!(
        "{:>5} - done",
        Instant::now().duration_since(now).as_millis()
    );
}
