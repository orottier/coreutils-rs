//! `xargs`: build and execute command lines from standard input
//!
//! Usage: `xargs [OPTION]... COMMAND [INITIAL-ARGS]...`
//!
//! Done:
//!  - read params from stdin, separated by SEP
//!  - execute commands
//!
//! Todo:
//!  - run each command with multiple input args
//!  - parallel execution
//!  - every thing else

use coreutils::{Input, InputArg};
use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::process::exit;
use std::process::Command;

use clap::{App, AppSettings, Arg};

const USAGE: &str = "build and execute command lines from standard input";

/// `xargs` payload
#[derive(Debug)]
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
}

impl<'a> Payload<'a> {
    fn to_command(&self, input: String) -> Command {
        let mut cmd = Command::new(self.command);
        self.initial_args.iter().for_each(|arg| {
            cmd.arg(arg);
        });
        cmd.arg(input);

        cmd
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

    let payload = Payload {
        command,
        initial_args,
        input_arg,
        verbose,
        input_sep,
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

    bufread
        .split(payload.input_sep)
        .flat_map(|line| line.ok())
        .flat_map(|line| String::from_utf8(line).ok())
        .try_for_each(|line| {
            let mut cmd = payload.to_command(line);
            if payload.verbose {
                eprintln!("{:?}", cmd);
            }

            match cmd.output() {
                Ok(output) => stdout
                    .write_all(&output.stdout)
                    .and_then(|_| stderr.write_all(&output.stderr))
                    .map(|_| ()),
                Err(e) => Err(e),
            }
        })
        .unwrap(); // todo, for borrow reasons do not return the io::Err

    Ok(())
}
