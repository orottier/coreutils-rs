//! `cut`: remove sections from each line of files
//!
//! With no FILE, or when FILE is -, read standard input.
//!
//! Use one, and only one of -b, -c or -f.  Each LIST is made up of one range, or many ranges
//! separated by commas.  Selected input is written in the same order that it is read, and is
//! written exactly once.  Each range is one of:
//!
//! - `N`      N'th byte, character or field, counted from 1
//! - `N-`     from N'th byte, character or field, to end of line
//! - `N-M`    from N'th to M'th (included) byte, character or field
//! - `-M`     from first to M'th (included) byte, character or field
//!
//! Todo:
//!  - allow multiple LISTs
//!  - multiple input files
//!  - line w/o a field separator should be printed verbatim
//!  - more options

use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::io::{Input, InputArg};

use clap::{App, Arg};

const USAGE: &str = "remove sections from each line of files";

struct Cut {
    start: usize,
    end: usize,
    unit: CutUnit,
}

enum CutUnit {
    /// cut on bytes
    Byte,
    /// cut on characters
    Character,
    /// cut on fields with given delimiter
    Field(String),
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let matches = App::new("tail")
        .about(USAGE)
        .arg(
            Arg::with_name("bytes")
                .short("b")
                .long("bytes")
                .value_name("LIST")
                .help("select only these bytes")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("characters")
                .short("c")
                .long("characters")
                .value_name("LIST")
                .help("select only these characters")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("fields")
                .short("f")
                .long("fields")
                .value_name("LIST")
                .help("select only these fields")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("delimiter")
                .short("d")
                .long("delimiter")
                .value_name("DELIM")
                .help("use DELIM instead of TAB for field delimiter")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("FILE")
                .help("input FILE(s), use - for stdin")
                //.multiple(true)
                .index(1),
        )
        .get_matches();

    let input_arg = match matches.value_of("FILE") {
        Some(s) if s != "-" => InputArg::File(s),
        _ => InputArg::Stdin,
    };

    let bytes = matches.value_of("bytes");
    let characters = matches.value_of("characters");
    let fields = matches.value_of("fields");

    let filled = [bytes, characters, fields];
    let mut filled = filled.iter().flatten();

    let range = match filled.next() {
        None => {
            eprintln!("Use one, and only one of -b, -c or -f.");
            exit(1);
        }
        Some(range) => range,
    };

    if filled.next().is_some() {
        eprintln!("Use one, and only one of -b, -c or -f.");
        exit(1);
    }

    let range_pieces: Vec<&str> = range.split('-').collect();
    let range_parsed = match range_pieces.as_slice() {
        [p] => p.parse().map(|v| (v, v)),
        [p, ""] => p.parse().map(|v| (v, usize::max_value() - 1)),
        ["", p] => p.parse().map(|v| (0, v)),
        [p, q] => match (p.parse(), q.parse()) {
            (Ok(p), Ok(q)) => Ok((p, q)),
            (Err(e), _) => Err(e),
            (_, Err(e)) => Err(e),
        },
        _ => {
            eprintln!("Invalid range specified");
            exit(1);
        }
    };

    let (start, end) = match range_parsed {
        Err(_) => {
            eprintln!("Invalid range specified");
            exit(1);
        }
        Ok(range) => range,
    };

    let unit = if bytes.is_some() {
        CutUnit::Byte
    } else if characters.is_some() {
        CutUnit::Character
    } else if fields.is_some() {
        let delim = matches.value_of("delimiter").unwrap_or("\t").to_string();
        CutUnit::Field(delim)
    } else {
        unreachable!()
    };

    let payload = Cut { unit, start, end };

    match cut(input_arg, payload) {
        Ok(_) => exit(0),
        Err(_) => exit(1),
    }
}

/// `cut` implementation
fn cut(input_arg: InputArg<&str>, cut: Cut) -> io::Result<()> {
    let input = Input::try_from(&input_arg)?;
    let lines = input.as_bufread().lines().filter_map(|l| l.ok());

    let Cut { start, end, unit } = cut; // deconstruct

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match unit {
        CutUnit::Character => lines.for_each(|line| {
            line.chars()
                .skip(start.saturating_sub(1))
                .take((end + 1).saturating_sub(start))
                .for_each(|c| write!(&mut stdout, "{}", c).unwrap());
            writeln!(&mut stdout).unwrap();
        }),
        CutUnit::Byte => lines.for_each(|line| {
            line.as_bytes()
                .iter()
                .skip(start.saturating_sub(1))
                .take((end + 1).saturating_sub(start))
                .for_each(|b| stdout.write_all(&[*b]).unwrap());
            writeln!(&mut stdout).unwrap();
        }),
        CutUnit::Field(delim) => lines.for_each(|line| {
            line.split(&delim)
                .skip(start.saturating_sub(1))
                .take((end + 1).saturating_sub(start))
                .for_each(|f| write!(&mut stdout, "{}{}", f, &delim).unwrap());
            writeln!(&mut stdout).unwrap();
        }),
    }

    Ok(())
}
