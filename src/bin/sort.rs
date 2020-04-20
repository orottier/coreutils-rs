//! `sort [OPTION]... [FILE]...`: sort lines of text files
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - code cleanup
//!  - more sort ordering
//!  - reverse
//!  - unique

use std::collections::BinaryHeap;
use std::convert::TryFrom;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::DerefMut;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use std::io::Split;
use std::vec::IntoIter;

use coreutils::executor::{Job, ThreadPool};
use coreutils::io::{Input, InputArg};

use clap::{App, Arg};
use tempfile::tempfile;

const USAGE: &str = "Sort lines of text files";
const N_WAY_MERGE: usize = 5;

type Line = Box<[u8]>;

/// `sort` payload
#[allow(dead_code)]
struct Payload<'a> {
    /// sort these inputs
    inputs: Vec<InputArg<&'a str>>,
    /// sort order
    sort_order: SortOrder,
    /// merge at most N inputs at once
    batch_size: NonZeroUsize,
    /// main memory buffer size
    buffer_size: Option<NonZeroUsize>,
    /// number of sorts run concurrently
    sort_threads: NonZeroU32,
}

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
    merged: bool,
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
            merged: false,
            source: LineIterator::Vec(iter),
            head: None,
        };

        me.next();

        me
    }

    pub fn is_flushed(&self) -> bool {
        if let LineIterator::File(_) = self.source {
            true
        } else {
            false
        }
    }

    pub fn is_merged(&self) -> bool {
        self.merged
    }

    pub fn peek(&self) -> Option<&Line> {
        self.head.as_ref()
    }

    pub fn drain<W: Write>(&mut self, write: &mut W) {
        self.for_each(|line| {
            write
                .write_all(&line)
                .and_then(|_| write.write_all(&[b'\n']))
                .unwrap();
        });
    }

    pub fn drain_until<W: Write>(&mut self, write: &mut W, limit: &Line) -> bool {
        while let Some(head) = self.next() {
            write
                .write_all(&head)
                .and_then(|_| write.write_all(&[b'\n']))
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

enum SortJob {
    Sort(Vec<Line>, Arc<Mutex<Vec<SortedChunk>>>, Instant),
    Merge(Vec<SortedChunk>, Arc<Mutex<Vec<SortedChunk>>>, Instant),
    MergeFlush(Vec<SortedChunk>, Arc<Mutex<Vec<SortedChunk>>>, Instant),
}

impl Job for SortJob {
    fn run(self) {
        match self {
            SortJob::Sort(lines, chunks, now) => run_sort(lines, chunks, now),
            SortJob::Merge(merge, chunks, now) => run_merge(merge, chunks, now),
            SortJob::MergeFlush(merge, chunks, now) => run_merge_flush(merge, chunks, now),
        }
    }
}

fn run_sort(mut lines: Vec<Line>, chunks: Arc<Mutex<Vec<SortedChunk>>>, now: Instant) {
    lines.sort_unstable();

    let chunk = SortedChunk::new(lines);
    chunks.lock().unwrap().push(chunk);

    eprintln!(
        "{:>5} threadpool - sorted",
        Instant::now().duration_since(now).as_millis()
    );
}

fn run_merge(merge: Vec<SortedChunk>, chunks: Arc<Mutex<Vec<SortedChunk>>>, now: Instant) {
    let mut merge: BinaryHeap<_> = merge.into_iter().collect();
    let mut merged = vec![];

    while let Some(mut chunk) = merge.pop() {
        match merge.peek().and_then(|c| c.peek()) {
            Some(limit) => {
                while let Some(head) = chunk.next() {
                    merged.push(head);
                    if chunk.peek().is_some() && chunk.peek().unwrap() > limit {
                        break;
                    }
                }

                if chunk.peek().is_some() {
                    merge.push(chunk);
                }
            }
            None => chunk.for_each(|l| merged.push(l)),
        }
    }

    let mut chunk = SortedChunk::new(merged);
    chunk.merged = true;

    chunks.lock().unwrap().push(chunk);

    eprintln!(
        "{:>5} threadpool - merged",
        Instant::now().duration_since(now).as_millis()
    );
}

fn run_merge_flush(merge: Vec<SortedChunk>, chunks: Arc<Mutex<Vec<SortedChunk>>>, now: Instant) {
    let mut merge: BinaryHeap<_> = merge.into_iter().collect();
    let mut tempfile = tempfile().unwrap();

    {
        let mut write = BufWriter::new(&mut tempfile);

        while let Some(mut chunk) = merge.pop() {
            let exhausted = match merge.peek().and_then(|c| c.peek()) {
                Some(limit) => chunk.drain_until(&mut write, limit),
                None => {
                    chunk.drain(&mut write);
                    true
                }
            };
            if !exhausted {
                merge.push(chunk);
            }
        }
    }

    tempfile.seek(SeekFrom::Start(0)).unwrap();

    let iter = BufReader::new(tempfile).split(b'\n');
    let mut chunk = SortedChunk {
        merged: true,
        source: LineIterator::File(iter),
        head: None,
    };
    chunk.next();

    chunks.lock().unwrap().push(chunk);

    eprintln!(
        "{:>5} threadpool - merge_flushed",
        Instant::now().duration_since(now).as_millis()
    );
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
            Arg::with_name("parallel")
                .long("parallel")
                .value_name("N")
                .help("change the number of sorts run concurrently to N")
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
        .unwrap_or_else(Vec::new);
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
        .and_then(NonZeroUsize::new)
        .unwrap_or_else(|| NonZeroUsize::new(100_000).unwrap());

    let buffer_size = matches
        .value_of("buffer-size")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(NonZeroUsize::new);

    let sort_threads = matches
        .value_of("parallel")
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .unwrap_or_else(|| NonZeroU32::new(num_cpus::get() as u32).unwrap());

    let payload = Payload {
        inputs,
        sort_order,
        batch_size,
        buffer_size,
        sort_threads,
    };

    sort(payload);
    exit(0)
}

/// `sort` handler
fn sort(payload: Payload) {
    let mut line_iter = payload
        .inputs
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

    let batch_size = payload.batch_size.get();
    let buffer_size = payload
        .buffer_size
        .map(|b| b.get())
        .unwrap_or(usize::max_value());
    eprintln!(
        "batch_size {}k, buffer_size {}M",
        batch_size >> 10,
        buffer_size >> 20
    );

    let mut executor = ThreadPool::new(payload.sort_threads);

    let mut exhausted = false;
    let mut bytes = 0;
    let mut batches = 0;
    let chunks = Arc::new(Mutex::new(vec![]));
    let now = Instant::now();

    while !exhausted {
        let mut lines = vec![];
        let mut line_count = 0;

        while line_count < batch_size && bytes < buffer_size {
            if let Some(line) = line_iter.next() {
                bytes += line.len();
                line_count += 1;
                lines.push(line);
            } else {
                exhausted = true;
                break;
            }
        }

        eprintln!(
            "{:>5} main - collected {} lines",
            Instant::now().duration_since(now).as_millis(),
            line_count
        );

        executor.submit(SortJob::Sort(lines, chunks.clone(), now));
        batches += 1;

        if !exhausted && (bytes > buffer_size || batches > N_WAY_MERGE) {
            batches = 0;
            let mut batch = vec![];
            let jobs = {
                let mut lock = chunks.lock().unwrap();
                let all = std::mem::replace(lock.deref_mut(), vec![]);

                let mut jobs = all
                    .into_iter()
                    .flat_map(|chunk| {
                        if chunk.is_flushed() || chunk.is_merged() {
                            lock.push(chunk);
                        } else {
                            batch.push(chunk);
                            if batch.len() == N_WAY_MERGE {
                                let payload = std::mem::replace(&mut batch, vec![]);
                                if bytes > buffer_size {
                                    return Some(SortJob::MergeFlush(payload, chunks.clone(), now));
                                } else {
                                    return Some(SortJob::Merge(payload, chunks.clone(), now));
                                }
                            }
                        }

                        None
                    })
                    .collect::<Vec<_>>();

                if !batch.is_empty() {
                    if bytes > buffer_size {
                        jobs.push(SortJob::MergeFlush(batch, chunks.clone(), now));
                    } else {
                        jobs.push(SortJob::Merge(batch, chunks.clone(), now));
                    }
                }

                jobs
            }; // drop chunks write lock

            // this could block if all workers are busy
            jobs.into_iter().for_each(|job| {
                eprintln!(
                    "{:>5} main - {} request",
                    Instant::now().duration_since(now).as_millis(),
                    match job {
                        SortJob::MergeFlush(..) => "merge_flush",
                        SortJob::Merge(..) => "merge",
                        _ => unreachable!(),
                    }
                );
                executor.submit(job)
            });

            bytes = 0;
        }
    }

    eprintln!(
        "{:>5} main - await thread pool",
        Instant::now().duration_since(now).as_millis()
    );
    executor.finish();

    eprintln!(
        "{:>5} main - done, merging {} chunks",
        Instant::now().duration_since(now).as_millis(),
        chunks.lock().unwrap().len()
    );

    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut stdout = BufWriter::new(stdout);
    let mut chunks: BinaryHeap<_> = chunks.lock().unwrap().drain(..).collect();

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
        "{:>5} main - done",
        Instant::now().duration_since(now).as_millis()
    );
}
