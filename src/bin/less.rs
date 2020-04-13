//! `less [FILE]`: opposite of more
//!
//! Less is a program similar to more (1), but which allows backward movement in the file as
//! well as forward movement. Also, less does not have to read the entire input file before
//! starting, so with large input files it starts up faster than text editors like vi (1)
//!
//! Todo:
//!  - handle terminal resize
//!  - handle stdin, handle appears to close after draining bufread
//!  - navigating: page up/down, search, tailing, etc
//!  - many other things

use termion::clear;
use termion::cursor;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use termion::terminal_size;

use memmap::{Mmap, MmapOptions};

use std::fs::File;
use std::io::{stdin, stdout, Write};
use std::process::exit;

use coreutils::{emit_bell, print_help_and_exit};

const USAGE: &str = "less <filename>: opposite of more";

struct Pager {
    mmap: Mmap,
    offset: usize,
}

impl Pager {
    fn new(mmap: Mmap) -> Self {
        Self { mmap, offset: 0 }
    }

    fn jump_to_top(&mut self) -> bool {
        if self.offset != 0 {
            self.offset = 0;
            true
        } else {
            false
        }
    }

    fn jump_to_bottom(&mut self) -> bool {
        if self.mmap.len() <= 1 {
            return false;
        }

        let mut height = 10; // todo
        let newline_pos = self.mmap[..(self.mmap.len() - 1)].iter().rposition(|c| {
            if *c == b'\n' {
                height -= 1;
            }
            height == 0
        });
        if let Some(index) = newline_pos {
            let new_offset = index + 1;
            if new_offset >= self.mmap.len() {
                false
            } else {
                self.offset = new_offset;
                true
            }
        } else {
            false
        }
    }

    fn scroll_down(&mut self) -> bool {
        let newline_pos = self.mmap[self.offset..].iter().position(|c| *c == b'\n');
        if let Some(index) = newline_pos {
            let new_offset = self.offset + index + 1;
            if new_offset >= self.mmap.len() {
                false
            } else {
                self.offset = new_offset;
                true
            }
        } else {
            false
        }
    }

    fn scroll_up(&mut self) -> bool {
        if self.offset <= 1 {
            return false;
        }

        let newline_pos = self.mmap[..(self.offset - 1)]
            .iter()
            .rposition(|c| *c == b'\n');
        if let Some(index) = newline_pos {
            let new_offset = index + 1;
            self.offset = new_offset;
        } else {
            self.offset = 0;
        }

        true
    }

    fn view(&self, width: u16, height: u16) -> impl Iterator<Item = &[u8]> + '_ {
        let chunk_size = width as usize;
        self.mmap[self.offset..]
            .split(|c| *c == b'\n')
            .flat_map(move |cs| cs.chunks(chunk_size))
            .chain(std::iter::repeat(&[b'~'][..]))
            .take(height as usize)
    }
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let filename = match args.next() {
        Some(s) if s.starts_with('-') => print_help_and_exit(USAGE),
        Some(s) => s,
        None => print_help_and_exit(USAGE),
    };

    match less(&filename) {
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
    pager: &Pager,
    size: (u16, u16),
) -> Result<(), Box<dyn std::error::Error>> {
    write!(stdout, "{}", clear::All)?;

    pager
        .view(size.0, size.1 - 1)
        .enumerate()
        .for_each(|(line_number, line_content)| {
            // place cursor on right line
            write!(stdout, "{}", cursor::Goto(1, line_number as u16 + 1)).unwrap();
            // write line
            let line = std::str::from_utf8(line_content).unwrap_or("invalid UTF8");
            write!(stdout, "{}", line).unwrap();
        });

    // write status
    write!(
        stdout,
        "{}terminal size w{} h{}",
        cursor::Goto(1, size.1),
        size.0,
        size.1
    )?;

    stdout.flush()?;

    Ok(())
}

/// `less` implementation
fn less(filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(filename)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };
    let mut pager = Pager::new(mmap);

    let mut stdout = AlternateScreen::from(stdout().into_raw_mode()?);
    write!(stdout, "{}", cursor::Hide)?;

    let stdin = stdin();
    let stdin = stdin.lock();

    let size = terminal_size()?;
    draw_screen(&mut stdout, &pager, size)?;

    for c in stdin.keys() {
        write!(stdout, "",)?;

        let redraw = match c? {
            Key::Char('q') => break,
            Key::Char('g') => pager.jump_to_top(),
            Key::Char('G') => pager.jump_to_bottom(),
            Key::Char(c) => {
                write!(
                    stdout,
                    "{}{}{}",
                    cursor::Goto(1, size.1),
                    clear::CurrentLine,
                    c
                )?;
                stdout.flush()?;
                false
            }
            Key::Esc => {
                write!(
                    stdout,
                    "{}{}ESC",
                    cursor::Goto(1, size.1),
                    clear::CurrentLine
                )?;
                stdout.flush()?;
                false
            }
            Key::Up => pager.scroll_up(),
            Key::Down => pager.scroll_down(),
            _ => false,
        };

        if redraw {
            draw_screen(&mut stdout, &pager, size)?;
        } else {
            emit_bell();
        }
    }

    write!(stdout, "{}", cursor::Show)?;

    Ok(())
}
