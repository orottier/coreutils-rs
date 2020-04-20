//! `timeout`: run a command with a time limit
//!
//! Usage: `timeout [OPTION] DURATION COMMAND [ARG]...`
//!
//! DURATION  is a floating point number with an optional suffix: 's' for seconds (the default),
//! 'm' for minutes, 'h' for hours or 'd' for days.  A duration of 0 disables the associated
//! timeout.
//!
//! If the command times out, and --preserve-status is not set, then exit with status 124.
//! Otherwise, exit with the status of COMMAND.  If no signal is specified,  send  the  TERM signal
//! upon  timeout.  The TERM signal kills any process that does not block or catch that signal.  It
//! may be necessary to use the KILL (9) signal, since this signal cannot be caught, in which case
//! the exit status is 128+9 rather than 124.
//!
//! Todo:
//!  - clean up child process (remains running even after main finishes)
//!  - send TERM signal instead of terminating thread
//!  - support more signals
//!  - send KILL if first signal has no effect
//!  - other options

use std::process::exit;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use coreutils::util::print_help_and_exit;

const USAGE: &str = "timeout [OPTION] DURATION COMMAND [ARG]...";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut duration_str = args.next().unwrap_or_else(|| print_help_and_exit(USAGE));
    let duration_unit = duration_str.pop().unwrap();
    let duration_val: f64 = duration_str
        .parse()
        .unwrap_or_else(|_| print_help_and_exit(USAGE));
    let duration = match duration_unit {
        's' => Duration::from_millis((duration_val * 1000.) as _),
        'm' => Duration::from_millis((duration_val * 1000. * 60.) as _),
        'h' => Duration::from_millis((duration_val * 1000. * 3600.) as _),
        'd' => Duration::from_millis((duration_val * 1000. * 3600. * 24.) as _),
        _ => print_help_and_exit(USAGE),
    };

    let command = args.next().unwrap_or_else(|| print_help_and_exit(USAGE));
    let args: Vec<_> = args.collect();

    let status_code = timeout(duration, &command, &args);
    exit(status_code)
}

/// `timeout` implementation
fn timeout(duration: Duration, command: &str, args: &[String]) -> i32 {
    let mut cmd = Command::new(command);
    args.iter().for_each(|arg| {
        cmd.arg(arg);
    });

    #[allow(clippy::mutex_atomic)] // we need it for the CondVar
    let pair = Arc::new((Mutex::new(true), Condvar::new()));
    let pair2 = pair.clone();

    let handle = thread::spawn(move || {
        let status_code = if let Ok(mut child) = cmd.spawn() {
            // todo, `wait()` closes stdin, is this desired?
            if let Ok(result) = child.wait() {
                result.code().unwrap_or(-1)
            } else {
                eprintln!("unable to await child process");
                -1
            }
        } else {
            eprintln!("unable to spawn command");
            -1
        };

        let (lock, cvar) = &*pair2;
        let mut pending = lock.lock().unwrap();
        *pending = false;

        // notify the condvar that the we are finished
        cvar.notify_one();

        status_code
    });

    let (lock, cvar) = &*pair;
    let result = cvar
        .wait_timeout_while(lock.lock().unwrap(), duration, |&mut pending| pending)
        .unwrap();

    if result.1.timed_out() {
        124
    } else {
        match handle.join() {
            Ok(status_code) => status_code,
            Err(_) => {
                eprintln!("unable to join thread");
                -1
            }
        }
    }
}
