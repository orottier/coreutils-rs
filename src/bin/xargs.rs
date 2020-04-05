//! `xargs`: build and execute command lines from standard input
//!
//! Usage: `xargs [OPTION]... COMMAND [INITIAL-ARGS]...`
//!
//! Done:
//!  - read params from stdin, separated by SEP
//!  - execute commands, with max_args input
//!
//! Todo:
//!  - support `-I {}`
//!  - less unwraps()
//!  - other options

use coreutils::{Input, InputArg};
use std::convert::TryFrom;
use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::process::exit;
use std::process::Command;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

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
    /// run at most MAX-PROCS processes at a time
    max_procs: NonZeroU32,
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

/// Convenience wrapper for a Command,
/// that writes its output to the main stdout and stderr
struct Job(Command);

impl Job {
    fn run(self) {
        let mut cmd = self.0;
        // we don't use spawn, but capture full output
        let output = cmd.output().expect("Unable to spawn command");

        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        let stderr = io::stderr();
        let mut stderr = stderr.lock();

        stdout
            .write_all(&output.stdout)
            .and_then(|_| stderr.write_all(&output.stderr))
            .expect("Error writing output");
    }
}

/// Threadpool implementation for parallel execution
struct ThreadPool {
    /// worker threads
    threads: Vec<JoinHandle<()>>,
    /// job submission channel, blocks if queue is full
    sender: SyncSender<Job>,
}

impl ThreadPool {
    fn new(size: NonZeroU32) -> Self {
        let size = size.get();
        let (sender, receiver) = sync_channel::<Job>(size as _);
        let receiver = Arc::new(Mutex::new(receiver));

        let threads = (0..size)
            .map(|_| {
                let thread_recv = receiver.clone();
                thread::spawn(move || {
                    // keep working, as long as we are able to `recv` jobs
                    while let Ok(job) = thread_recv.lock().unwrap().recv() {
                        job.run();
                    }
                })
            })
            .collect();

        Self { threads, sender }
    }

    fn submit(&mut self, job: Job) {
        self.sender.send(job).unwrap()
    }

    fn finish(self) {
        let ThreadPool {
            threads, sender, ..
        } = self;
        drop(sender); // signal receivers they can exit

        threads
            .into_iter()
            .for_each(|thread| thread.join().unwrap())
    }
}

/// Executor for running command batches
enum Executor {
    /// single-threaded, no overhead
    Synchronous,
    /// multi-threaded
    Concurrent(ThreadPool),
}

impl Executor {
    fn submit(&mut self, cmd: Command) {
        let job = Job(cmd);
        match self {
            Executor::Synchronous => job.run(), // run immediately
            Executor::Concurrent(thread_pool) => thread_pool.submit(job),
        }
    }

    fn finish(self) {
        match self {
            Executor::Synchronous => (),
            Executor::Concurrent(thread_pool) => thread_pool.finish(),
        }
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
            Arg::with_name("max-procs")
                .short("P")
                .long("max-procs")
                .value_name("MAX-PROCS")
                .help("run at most MAX-PROCS processes at a time")
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
        .unwrap_or_else(|| vec![]);
    let verbose = matches.is_present("verbose");
    let input_sep = matches.value_of("null").map(|_| 0x0).unwrap_or(b'\n');
    let max_args = matches
        .value_of("max-args")
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .unwrap_or_else(|| NonZeroU32::new(5000).unwrap());
    let max_procs = matches
        .value_of("max-procs")
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .unwrap_or_else(|| NonZeroU32::new(1).unwrap());

    let payload = Payload {
        command,
        initial_args,
        input_arg,
        verbose,
        input_sep,
        max_args,
        max_procs,
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

    let mut iter = bufread
        .split(payload.input_sep)
        .flat_map(|line| line.ok())
        .flat_map(|line| String::from_utf8(line).ok());

    let mut executor = match payload.max_procs.get() {
        1 => Executor::Synchronous,
        _ => Executor::Concurrent(ThreadPool::new(payload.max_procs)),
    };

    while let Some(cmd) = payload.to_command(&mut iter) {
        if payload.verbose {
            eprintln!("{:?}", cmd);
        }

        executor.submit(cmd);
    }

    executor.finish();
    Ok(())
}
