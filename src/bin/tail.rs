//! `tail`: Print the last 10 lines of each FILE to standard output.
//!
//! With more than one FILE, precede each with a header giving the file name.
//! With no FILE, or when FILE is -, read standard input.
//!
//! Todo:
//!  - tail multiple files

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::process::exit;
use std::thread;
use std::time::Duration;

use coreutils::io::InputArg;

use clap::{App, Arg};

const USAGE: &str = "Print the last 10 lines of each FILE to standard output";
const DEFAULT_BUF_SIZE: u64 = 2048;

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let matches = App::new("tail")
        .about(USAGE)
        .arg(
            Arg::with_name("follow")
                .long("follow")
                .short("f")
                .help("output appended data as the file grow"),
        )
        .arg(
            Arg::with_name("lines")
                .short("n")
                .long("lines")
                .value_name("NUM")
                .help("output the last NUM lines, instead of the last 10")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("FILE")
                .help("tail this FILE, use - for stdin")
                //.multiple(true)
                .index(1),
        )
        .get_matches();

    let input_arg = match matches.value_of("FILE") {
        Some(s) if s != "-" => InputArg::File(s),
        _ => InputArg::Stdin,
    };

    let lines = matches
        .value_of("lines")
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(10);

    let follow = matches.is_present("follow");

    let result = match input_arg {
        InputArg::File(filename) => tail_filename(filename, lines, follow),
        InputArg::Stdin => tail_stdin(lines),
    };

    match result {
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    };
}

/// `tail` implementation for stdin
fn tail_stdin(lines: usize) -> io::Result<()> {
    let stdin = io::stdin();
    let stdin = stdin.lock();

    let mut buffer = VecDeque::with_capacity(lines);
    stdin.lines().filter_map(|line| line.ok()).for_each(|line| {
        if buffer.len() == lines {
            buffer.pop_front();
        }
        buffer.push_back(line);
    });

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    buffer
        .into_iter()
        .try_for_each(|line| writeln!(&mut stdout, "{}", line))
}

/// `tail` implementation for filename
fn tail_filename(filename: &str, lines: usize, follow: bool) -> io::Result<()> {
    let mut file = File::open(filename)?;
    let initial_position = file.seek(SeekFrom::End(0))?;

    let mut lines_found = 0;
    let mut tail = vec![];

    // read N lines back
    let mut position = initial_position;
    loop {
        let new_position = if position < DEFAULT_BUF_SIZE {
            0
        } else {
            position - DEFAULT_BUF_SIZE
        };
        let length = (position - new_position) as usize;
        position = new_position;

        file.seek(SeekFrom::Start(position))?;
        let mut buf = vec![0; length];

        file.read_exact(&mut buf)?;

        let mut iter = buf.iter().enumerate();
        while let Some((pos, c)) = iter.next_back() {
            if *c == b'\n' {
                lines_found += 1;
                if lines_found > lines {
                    buf = buf.into_iter().skip(pos + 1).collect();
                    break;
                }
            }
        }

        tail.push(buf);

        if position == 0 || lines_found > lines {
            break;
        }
    }

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    while let Some(chunk) = tail.pop() {
        stdout.write_all(&chunk)?;
    }

    if follow {
        file.seek(SeekFrom::Start(initial_position))?;
        loop {
            let bytes = std::io::copy(&mut file, &mut stdout)?;
            if bytes == 0 {
                thread::sleep(Duration::from_millis(1000))
            } else if bytes < DEFAULT_BUF_SIZE {
                thread::sleep(Duration::from_millis(200));
            }
        }
    }

    Ok(())
}
