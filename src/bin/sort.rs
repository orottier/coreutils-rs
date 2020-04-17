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
use std::io::{self, BufRead, BufReader, BufWriter, Seek, SeekFrom, Split, Write};
use std::num::NonZeroU32;
use std::process::exit;

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

struct SortedChunk {
    source: Split<BufReader<File>>,
    head: Option<Line>,
}

impl Iterator for SortedChunk {
    type Item = Line;

    fn next(&mut self) -> Option<Line> {
        let next = self.source.next().map(|l| l.unwrap().into_boxed_slice());
        std::mem::replace(&mut self.head, next)
    }
}

impl SortedChunk {
    pub fn file(file: File) -> Self {
        let mut me = Self {
            source: BufReader::new(file).split(b'\n'),
            head: None,
        };
        me.next();

        me
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
            //eprintln!("head {} limit {}", std::str::from_utf8(&head).unwrap(), std::str::from_utf8(&limit).unwrap());

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
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(NonZeroU32::new);

    sort(&inputs[..], sort_order, batch_size);
    exit(0)
}

/// `sort` handler
fn sort(input_args: &[InputArg<&str>], _sort_order: SortOrder, batch_size: Option<NonZeroU32>) {
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

    match batch_size {
        None => sort_in_mem(line_iter),
        Some(batch_size) => sort_external(line_iter, batch_size),
    }
}

fn sort_in_mem(mut line_iter: impl Iterator<Item = Line>) {
    let mut lines: Vec<_> = line_iter.collect();
    lines.sort_unstable();

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    lines
        .into_iter()
        .try_for_each(|line| {
            stdout
                .write_all(&line)
                .and_then(|_| stdout.write_all(&[b'\n']))
        })
        .unwrap();
}

fn sort_external(mut line_iter: impl Iterator<Item = Line>, batch_size: NonZeroU32) {
    let batch_size = batch_size.get();
    eprintln!("running with batch_size {}", batch_size);

    let mut exhausted = false;
    let mut files = vec![];
    let now = std::time::Instant::now();

    let mut lines = vec![];
    while !exhausted {
        let mut count = 0;

        while count < batch_size {
            if let Some(line) = line_iter.next() {
                lines.push(line);
            } else {
                exhausted = true;
                break;
            }
            count += 1;
        }

        eprintln!(
            "{} - collected {} lines",
            std::time::Instant::now().duration_since(now).as_millis(),
            count
        );
        lines.sort_unstable();
        eprintln!(
            "{} - sorted",
            std::time::Instant::now().duration_since(now).as_millis()
        );

        let mut tempfile = tempfile().unwrap();
        {
            let mut write = BufWriter::new(&mut tempfile);
            lines
                .iter()
                .try_for_each(|line| {
                    write
                        .write_all(line)
                        .and_then(|_| write.write_all(&[b'\n']))
                })
                .unwrap();
        }

        eprintln!(
            "{} - written tempfile {:?}",
            std::time::Instant::now().duration_since(now).as_millis(),
            tempfile
        );
        files.push(tempfile);

        lines.clear(); // re-use allocated vec
    }

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let mut chunks: BinaryHeap<_> = files
        .into_iter()
        .map(|mut tempfile| {
            tempfile.seek(SeekFrom::Start(0)).unwrap();
            SortedChunk::file(tempfile)
        })
        .collect();

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
        "{} - done",
        std::time::Instant::now().duration_since(now).as_millis()
    );
}
