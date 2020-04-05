//! `xargs`: build and execute command lines from standard input
//!
//! Usage: `xargs [OPTION]... COMMAND [INITIAL-ARGS]...`
//!
//! Done:
//!  - read params from stdin, separated by SEP
//!  - execute commands, with max_args input
//!
//! Todo:
//!  - parallel execution
//!  - everything else

use coreutils::{Input, InputArg};
use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::process::exit;
use std::process::Command;

use clap::{App, AppSettings, Arg};

const USAGE: &str = "build and execute command lines from standard input";

/// `xargs` payload
struct Payload<'a> {
    /// run this command
    command: &'a str,
    /// run command with these initial arguments
    initial_args: Vec<&'a str>,
    /// run command on this input
    input_arg: InputArg<&'a str>,
    /// print commands before executing them
    verbose: bool,
    /// separator of items in input stream
    input_sep: u8,
    /// use at most MAX-ARGS arguments per command line
    max_args: NonZeroU32,
}

impl<'a> Payload<'a> {
    fn to_command(&self, input: &mut dyn Iterator<Item = String>) -> Option<Command> {
        let mut cmd = Command::new(self.command);

        self.initial_args.iter().for_each(|arg| {
            cmd.arg(arg);
        });

        let mut exhausted = true;
        input.take(self.max_args.get() as _).for_each(|arg| {
            exhausted = false;
            cmd.arg(arg);
        });

        if exhausted {
            return None;
        }

        Some(cmd)
    }
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let matches = App::new("xargs")
        .setting(AppSettings::TrailingVarArg)
        .about(USAGE)
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .short("t")
                .help("print commands before executing them"),
        )
        .arg(
            Arg::with_name("null")
                .long("null")
                .short("0")
                .help("items are separated by a null, not whitespace"),
        )
        .arg(
            Arg::with_name("arg-file")
                .short("a")
                .long("arg-file")
                .value_name("FILE")
                .help("read arguments from FILE, not standard input")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("max-args")
                .short("n")
                .long("max-args")
                .value_name("MAX-ARGS")
                .help("use at most MAX-ARGS arguments per command line")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("COMMAND")
                .help("run this command")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("INITIAL_ARGS")
                .help("Run COMMAND with arguments INITIAL-ARGS")
                .multiple(true)
                .index(2),
        )
        .get_matches();

    let input_arg = match matches.value_of("arg-file") {
        Some(s) => InputArg::File(s),
        None => InputArg::Stdin,
    };

    let command = matches.value_of("COMMAND").unwrap();
    let initial_args = matches
        .values_of("INITIAL_ARGS")
        .map(|values| values.collect())
        .unwrap_or(vec![]);
    let verbose = matches.is_present("verbose");
    let input_sep = matches.value_of("null").map(|_| 0x0).unwrap_or(b'\n');
    let max_args = matches
        .value_of("max-args")
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(|n| NonZeroU32::new(n))
        .unwrap_or_else(|| NonZeroU32::new(5000).unwrap());

    let payload = Payload {
        command,
        initial_args,
        input_arg,
        verbose,
        input_sep,
        max_args,
    };

    match xargs(payload) {
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// `xargs` implementation
fn xargs(payload: Payload) -> io::Result<()> {
    let input = Input::try_from(&payload.input_arg)?;
    let bufread = input.as_bufread();

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let stderr = io::stderr();
    let mut stderr = stderr.lock();

    let mut iter = bufread
        .split(payload.input_sep)
        .flat_map(|line| line.ok())
        .flat_map(|line| String::from_utf8(line).ok());

    loop {
        let mut cmd = match payload.to_command(&mut iter) {
            Some(cmd) => cmd,
            None => break,
        };

        if payload.verbose {
            eprintln!("{:?}", cmd);
        }

        let output = cmd.output()?;
        stdout
            .write_all(&output.stdout)
            .and_then(|_| stderr.write_all(&output.stderr))?;
    }

    Ok(())
}
