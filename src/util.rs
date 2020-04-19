//! Miscellaneous helper functions

use std::io::{self, Write};
use std::process::exit;

pub fn print_help_and_exit(usage: &str) -> ! {
    eprintln!("{}", usage);
    exit(1);
}

pub fn emit_bell() {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(&[0x07]);
    let _ = stdout.flush(); // needed since no newline was written
}
