//! `pv`: monitor the progress of data through a pipe
//!
//! pv  shows  the  progress  of data through a pipeline by giving information such as time
//! elapsed, percentage completed (with progress bar), current throughput rate, total data
//! transferred, and ETA.
//!
//! Todo:
//!  - support options

use std::cmp::min;
use std::convert::TryFrom;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::exit;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use coreutils::{print_help_and_exit, Input, InputArg};

use indicatif::{ProgressBar, ProgressStyle};

const USAGE: &str =
    "pv [OPTION] [FILE]...\npv [-h|-V]\nmonitor the progress of data through a pipe";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let input_arg = match args.next() {
        Some(s) => InputArg::File(s),
        None => InputArg::Stdin,
    };

    match pv(&input_arg) {
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// `pv` implementation
fn pv(input_arg: &InputArg) -> io::Result<()> {
    let input = Input::try_from(input_arg)?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let pb = match input_arg {
        InputArg::Stdin => {
            let pb = ProgressBar::new_spinner();
            let style = ProgressStyle::default_bar()
                .template(" [{bytes_per_sec}] [{elapsed_precise}] {bytes}/?");
            pb.set_style(style);
            pb
        }
        InputArg::File(filename) => {
            let metadata = fs::metadata(filename)?;
            let pb = ProgressBar::new(metadata.len());
            let style = ProgressStyle::default_bar()
                .template(" [{bytes_per_sec}] [{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})")
                .progress_chars("#>-");
            pb.set_style(style);
            pb
        }
    };

    let mut bufread = input.as_bufread();
    let mut bytes_read = 0u64;
    let mut last_draw = UNIX_EPOCH;

    loop {
        let buffer = bufread.fill_buf()?;
        if buffer.is_empty() {
            break;
        }

        let length = buffer.len();
        bytes_read += length as u64;

        let now = SystemTime::now();
        if now.duration_since(last_draw).unwrap().as_millis() > 500 {
            pb.set_position(bytes_read);
            last_draw = now;
        }

        stdout.write_all(buffer)?;
        bufread.consume(length);
    }

    pb.finish_with_message("downloaded");

    Ok(())
}
