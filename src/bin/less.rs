//! `less [FILE]`: opposite of more
//!
//! Less is a program similar to more (1), but which allows backward movement in the file as
//! well as forward movement. Also, less does not have to read the entire input file before
//! starting, so with large input files it starts up faster than text editors like vi (1)
//!
//! Todo:
//!  - use memmap or clever seeking for large inputs
//!  - handle stdin, handle appears to close after draining bufread
//!  - better line wrapping
//!  - navigating: page up/down, search, tailing, etc
//!  - many other things

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use termion::terminal_size;

use std::convert::TryFrom;
use std::io::{stdin, stdout, BufRead, Write};
use std::process::exit;

use coreutils::{print_help_and_exit, Input, InputArg};

const USAGE: &str = "less [FILE]: opposite of more";

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let input_arg = match args.next() {
        Some(s) if s.starts_with('-') => print_help_and_exit(USAGE),
        Some(s) => InputArg::File(s),
        None => InputArg::Stdin,
    };

    match less(&input_arg) {
        Ok(_) => exit(0),
        Err(e) => {
            eprintln!("{:?}", e);
            exit(1)
        }
    }
}

/// Fill current screen with file content (with offset)
fn draw_screen(
    stdout: &mut AlternateScreen<RawTerminal<std::io::Stdout>>,
    lines: &[String],
    offset: usize,
    size: (u16, u16),
) -> Result<(), Box<dyn std::error::Error>> {
    write!(stdout, "{}", termion::clear::All)?;

    let term_width = size.0 as usize;
    let max_lines = size.1 as usize - 1;

    let mut term_line = 0;
    let mut line_cursor = 0;
    while term_line < max_lines {
        // place cursor on right line
        write!(stdout, "{}", termion::cursor::Goto(1, term_line as u16 + 1))?;

        match lines.get(offset + line_cursor) {
            None => {
                write!(stdout, "~")?;
                term_line += 1;
            }
            Some(line) => {
                for start_slice in (0..line.len()).step_by(term_width) {
                    let end_slice = (start_slice + term_width).min(line.len());
                    write!(stdout, "{}", &line[start_slice..end_slice])?;
                    term_line += 1;

                    if term_line >= max_lines {
                        break;
                    }
                    write!(stdout, "{}", termion::cursor::Goto(1, term_line as u16 + 1))?;
                }
                line_cursor += 1;
            }
        };
    }

    // write status
    write!(
        stdout,
        "{}terminal size w{} h{}",
        termion::cursor::Goto(1, size.1),
        size.0,
        size.1
    )?;

    stdout.flush()?;

    Ok(())
}

/// `less` implementation
fn less(input_arg: &InputArg<String>) -> Result<(), Box<dyn std::error::Error>> {
    let input = Input::try_from(input_arg)?;
    let lines: Vec<_> = input
        .as_bufread()
        .lines()
        .filter_map(|line| line.ok())
        .collect();

    let mut stdout = AlternateScreen::from(stdout().into_raw_mode()?);
    write!(stdout, "{}", termion::cursor::Hide)?;

    let stdin = stdin();
    let stdin = stdin.lock();

    let size = terminal_size()?;
    let mut offset = 0;
    draw_screen(&mut stdout, &lines, offset, size)?;

    for c in stdin.keys() {
        write!(
            stdout,
            "{}{}",
            termion::cursor::Goto(1, size.1),
            termion::clear::CurrentLine
        )?;

        match c? {
            Key::Char('q') => break,
            Key::Char(c) => write!(stdout, "{}", c)?,
            Key::Alt(c) => write!(stdout, "^{}", c)?,
            Key::Ctrl(c) => write!(stdout, "*{}", c)?,
            Key::Esc => write!(stdout, "ESC")?,
            Key::Up => {
                if offset > 0 {
                    offset -= 1;
                    draw_screen(&mut stdout, &lines, offset, size)?;
                }
            }
            Key::Down => {
                if offset < lines.len() {
                    offset += 1;
                    draw_screen(&mut stdout, &lines, offset, size)?;
                }
            }
            _ => {}
        }
        stdout.flush()?;
    }

    write!(stdout, "{}", termion::cursor::Show)?;

    Ok(())
}
