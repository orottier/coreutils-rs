//! `tee [OPTION]... [FILE]...`: read from standard input and write to standard output and files
//!
//! The tee utility copies standard input to standard output, making a copy in zero or more files.
//! The output is unbuffered.
//!
//! Todo:
//!  - use `-i` to ignore the SIGINT signal

use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;

use coreutils::{print_help_and_exit, Output, OutputArg};

const USAGE: &str =
    "tee [OPTION]... [FILE]...\nread from standard input and write to standard output and files";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut output_args = vec![];

    let append = match args.next() {
        Some(s) if &s == "-a" => true,
        Some(s) if s.starts_with('-') => print_help_and_exit(USAGE),
        Some(s) => (output_args.push(OutputArg::File(s, false)), false).1,
        None => false,
    };

    output_args.extend(args.map(|s| OutputArg::File(s, append)));

    match tee(&output_args) {
        Ok(_) => exit(0),
        Err(_) => exit(1),
    }
}

/// `tee` implementation
fn tee(output_args: &[OutputArg<String>]) -> io::Result<()> {
    let mut outputs = output_args
        .iter()
        .map(Output::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    // add Stdout to outputs
    outputs.push(Output::Stdout(io::stdout()));

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    loop {
        let buffer = stdin.fill_buf()?;
        if buffer.is_empty() {
            break;
        }

        outputs
            .iter_mut()
            .try_for_each(|write| write.write_all(buffer))?;

        let length = buffer.len();
        stdin.consume(length);
    }

    Ok(())
}
