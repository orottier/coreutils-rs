//! `wc`: print newline, word, and byte counts for each file
//!
//! Print newline, word, and byte counts for each FILE, and a total line if more than one FILE is
//! specified.  A word is a non-zero-length sequence of characters delimited by white space.
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! This executable counts chunks of bytes in parallel, taking care of proper multibyte char
//! boundaries. Special care is taken to not exhaust memory when processing huge single-line files.
//!
//! Todo:
//!  - print info for each input, not just grand total

use std::convert::TryFrom;
use std::io::{BufReader, Read};
use std::num::NonZeroU32;
use std::process::exit;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use coreutils::chunks::{ChunkedItem, ChunkedReader};
use coreutils::executor::{Job, ThreadPool};
use coreutils::io::{Input, InputArg};
use coreutils::util::print_help_and_exit;

const USAGE: &str = "cat [OPTION]... [FILE]... : concatenate FILE(s) to standard output";

/// Process chunks of 1MB each
const CHUNK_SIZE: usize = 1 << 20;

// https://tools.ietf.org/html/rfc3629
static UTF8_CHAR_WIDTH: [u8; 256] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x1F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x3F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x5F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x7F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0x9F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0xBF
    0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, // 0xDF
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, // 0xEF
    4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xFF
];

/// Stream bytes into chars
struct BytesToChars<I: Iterator<Item = u8>> {
    bytes: I,
}

/// Unicode point, could be invalid
struct UncheckedChar([u8; 4], usize);

impl<I: Iterator<Item = u8>> Iterator for BytesToChars<I> {
    type Item = UncheckedChar;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = [0u8; 4];

        buf[0] = match self.bytes.next() {
            None => return None,
            Some(b) => b,
        };

        let len = UTF8_CHAR_WIDTH[buf[0] as usize] as usize;
        if len == 0 {
            panic!("invalid unicode");
        }

        for b in buf.iter_mut().take(len).skip(1) {
            *b = self.bytes.next().expect("incomplete unicode char");
        }

        Some(UncheckedChar(buf, len))
    }
}

/// Count bytes and newlines
fn count_bytes<I: Iterator<Item = u8>>(input: I) -> (u64, u64) {
    let mut bytes = 0;
    let mut lines = 0;

    for b in input {
        bytes += 1;
        if b == b'\n' {
            lines += 1;
        }
    }

    // count the newline at the end of this chunk
    bytes += 1;
    lines += 1;

    (bytes, lines)
}

/// Count all `wc` quantities
fn count_all<I: Iterator<Item = UncheckedChar>>(input: I) -> (u64, u64, u64, u64, u64) {
    let mut bytes = 0;
    let mut chars = 0;
    let mut lines = 0;
    let mut words = 0;
    let mut max_line_length = 0;

    let mut line_length = 0;
    let mut prev_was_whitespace = true;
    let mut prev_was_newline = false;

    for UncheckedChar(buf, len) in input {
        bytes += len as u64;
        chars += 1;

        // only consider ASCII whitespace for now
        if len == 1 && buf[0] >= 9 && buf[0] <= 14 {
            if !prev_was_whitespace {
                words += 1;
                prev_was_whitespace = true;
            }

            if buf[0] == 10 {
                if line_length > max_line_length {
                    max_line_length = line_length;
                }

                lines += 1;
                line_length = 0;
                prev_was_newline = true;
            } else {
                line_length += 1;
                prev_was_newline = false;
            }
        } else {
            prev_was_whitespace = false;
            prev_was_newline = false;
            line_length += 1;
        }
    }

    if !prev_was_newline {
        chars += 1;
        bytes += 1;
        lines += 1;
    }

    if !prev_was_whitespace {
        words += 1;
    }

    (bytes, chars, words, lines, max_line_length)
}

struct WcBytesJob {
    chunk: ChunkedItem<Box<dyn Read + Send>>,

    bytes: Arc<AtomicU64>,
    lines: Arc<AtomicU64>,
}

impl Job for WcBytesJob {
    fn run(self) {
        let (b, l) = match self.chunk {
            ChunkedItem::Chunk(data) => count_bytes(data.into_iter()),
            ChunkedItem::Bail(data, read) => {
                let bytes_left = BufReader::new(read).bytes().map(|b| b.unwrap());
                let bytes = data.into_iter().chain(bytes_left);
                count_bytes(bytes)
            }
        };

        self.bytes.fetch_add(b, Ordering::Relaxed);
        self.lines.fetch_add(l, Ordering::Relaxed);
    }
}

struct WcAllJob {
    chunk: ChunkedItem<Box<dyn Read + Send>>,

    bytes: Arc<AtomicU64>,
    chars: Arc<AtomicU64>,
    words: Arc<AtomicU64>,
    lines: Arc<AtomicU64>,
    max_line_length: Arc<AtomicU64>,
}

impl Job for WcAllJob {
    fn run(self) {
        let (b, c, w, l, ll) = match self.chunk {
            ChunkedItem::Chunk(data) => {
                let chars = BytesToChars {
                    bytes: data.into_iter(),
                };
                count_all(chars)
            }
            ChunkedItem::Bail(data, read) => {
                let bytes_left = BufReader::new(read).bytes().map(|b| b.unwrap());
                let bytes = data.into_iter().chain(bytes_left);
                let chars = BytesToChars { bytes };
                count_all(chars)
            }
        };

        self.bytes.fetch_add(b, Ordering::Relaxed);
        self.chars.fetch_add(c, Ordering::Relaxed);
        self.words.fetch_add(w, Ordering::Relaxed);
        self.lines.fetch_add(l, Ordering::Relaxed);
        self.max_line_length.fetch_add(ll, Ordering::Relaxed);
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

    if chars || words || max_line_length {
        wc_all(&inputs, bytes, chars, words, lines, max_line_length);
    } else {
        wc_bytes(&inputs, bytes, lines);
    }

    exit(0)
}

/// `wc` implementation, counting only bytes and/or lines
fn wc_bytes(input_args: &[InputArg<String>], show_bytes: bool, show_lines: bool) {
    let bytes_total = Arc::new(AtomicU64::new(0));
    let lines_total = Arc::new(AtomicU64::new(0));

    let mut executor = ThreadPool::new(NonZeroU32::new(num_cpus::get() as u32).unwrap());

    input_args
        .iter()
        .flat_map(|input_arg| {
            Input::try_from(input_arg)
                .map_err(|_| eprintln!("Unable to read {:?}", input_arg))
                .ok()
        })
        .map(|input| input.into_read())
        .for_each(|read| {
            let reader = ChunkedReader::new(read, b'\n', CHUNK_SIZE);
            reader.for_each(|chunk| {
                let job = WcBytesJob {
                    chunk: chunk.unwrap(),
                    bytes: bytes_total.clone(),
                    lines: lines_total.clone(),
                };
                executor.submit(job);
            });
        });

    executor.finish();

    if show_lines {
        print!("l:{} ", lines_total.load(Ordering::Relaxed));
    }
    if show_bytes {
        print!("b:{} ", bytes_total.load(Ordering::Relaxed));
    }
    println!()
}

/// `wc` implementation, counting all quantities
fn wc_all(
    input_args: &[InputArg<String>],
    show_bytes: bool,
    show_chars: bool,
    show_words: bool,
    show_lines: bool,
    show_max_line_length: bool,
) {
    let bytes_total = Arc::new(AtomicU64::new(0));
    let chars_total = Arc::new(AtomicU64::new(0));
    let words_total = Arc::new(AtomicU64::new(0));
    let lines_total = Arc::new(AtomicU64::new(0));
    let max_line_length = Arc::new(AtomicU64::new(0));

    let mut executor = ThreadPool::new(NonZeroU32::new(num_cpus::get() as u32).unwrap());

    input_args
        .iter()
        .flat_map(|input_arg| {
            Input::try_from(input_arg)
                .map_err(|_| eprintln!("Unable to read {:?}", input_arg))
                .ok()
        })
        .map(|input| input.into_read())
        .for_each(|read| {
            let reader = ChunkedReader::new(read, b'\n', CHUNK_SIZE);
            reader.for_each(|chunk| {
                let job = WcAllJob {
                    chunk: chunk.unwrap(),
                    bytes: bytes_total.clone(),
                    chars: chars_total.clone(),
                    words: words_total.clone(),
                    lines: lines_total.clone(),
                    max_line_length: max_line_length.clone(),
                };
                executor.submit(job);
            });
        });

    executor.finish();

    if show_lines {
        print!("l:{} ", lines_total.load(Ordering::Relaxed));
    }
    if show_words {
        print!("w:{} ", words_total.load(Ordering::Relaxed));
    }
    if show_chars {
        print!("c:{} ", chars_total.load(Ordering::Relaxed));
    }
    if show_bytes {
        print!("b:{} ", bytes_total.load(Ordering::Relaxed));
    }
    if show_max_line_length {
        print!("b:{} ", max_line_length.load(Ordering::Relaxed));
    }
    println!()
}
