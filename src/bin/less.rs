//! `less [FILE]`: opposite of more
//!
//! Less is a program similar to more (1), but which allows backward movement in the file as
//! well as forward movement. Also, less does not have to read the entire input file before
//! starting, so with large input files it starts up faster than text editors like vi (1)
//!
//! Todo:
//!  - search backwards
//!  - handle terminal resize
//!  - handle stdin, handle appears to close after draining bufread
//!  - page up/down, tailing, etc
//!  - many other things

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::screen::AlternateScreen;
use termion::{clear, color, cursor, terminal_size};

use memmap::{Mmap, MmapOptions};
use regex::bytes::Regex;

use std::fs::File;
use std::io::{stdin, stdout, Write};
use std::process::exit;

use coreutils::{emit_bell, print_help_and_exit};

const USAGE: &str = "less <filename>: opposite of more";

struct Pager {
    mmap: Mmap,
    size: (u16, u16),
    scroll_pos: usize,
    cursor: usize,
    search: Option<Regex>,
}

impl Pager {
    fn new(mmap: Mmap, size: (u16, u16)) -> Self {
        Self {
            mmap,
            size,
            scroll_pos: 0,
            cursor: 0,
            search: None,
        }
    }

    fn jump_to_top(&mut self) -> bool {
        if self.scroll_pos != 0 {
            self.scroll_pos = 0;
            self.cursor = 0;
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
            let new_scroll_pos = index + 1;
            if new_scroll_pos >= self.mmap.len() {
                false
            } else {
                self.scroll_pos = new_scroll_pos;
                self.cursor = new_scroll_pos;
                true
            }
        } else {
            false
        }
    }

    fn scroll_down(&mut self) -> bool {
        let newline_pos = self.mmap[self.scroll_pos..]
            .iter()
            .position(|c| *c == b'\n');
        if let Some(index) = newline_pos {
            let new_scroll_pos = self.scroll_pos + index + 1;
            if new_scroll_pos >= self.mmap.len() {
                false
            } else {
                self.scroll_pos = new_scroll_pos;
                self.cursor = new_scroll_pos;
                true
            }
        } else {
            false
        }
    }

    fn scroll_up(&mut self) -> bool {
        if self.scroll_pos <= 1 {
            return false;
        }

        let newline_pos = self.mmap[..(self.scroll_pos - 1)]
            .iter()
            .rposition(|c| *c == b'\n');
        if let Some(index) = newline_pos {
            let new_scroll_pos = index + 1;
            self.scroll_pos = new_scroll_pos;
            self.cursor = new_scroll_pos;
        } else {
            self.scroll_pos = 0;
            self.cursor = 0;
        }

        true
    }

    fn search(&mut self, search: &str) -> bool {
        self.search = Regex::new(search).ok();
        self.cursor = self.scroll_pos;
        self.search_next();

        true // always redraw since the search query may change
    }

    fn search_next(&mut self) -> bool {
        let regex = match &self.search {
            None => return false,
            Some(regex) => regex,
        };

        if let Some(mat) = regex.find(&self.mmap[(self.cursor + 1)..]) {
            let new_pos = self.cursor + mat.start() + 1;
            self.scroll_pos = new_pos;
            self.scroll_up();
            self.cursor = new_pos;
            true
        } else {
            false
        }
    }

    fn search_prev(&mut self) -> bool {
        let _regex = match &self.search {
            None => return false,
            Some(regex) => regex,
        };

        todo!()
    }

    fn draw_onto(
        &self,
        stdout: &mut AlternateScreen<RawTerminal<std::io::Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        write!(stdout, "{}", clear::All)?;

        let chunk_size = self.size.0 as usize;
        let height = self.size.1 as usize;

        self.mmap[self.scroll_pos..]
            .split(|c| *c == b'\n')
            .flat_map(move |cs| {
                if cs.is_empty() {
                    // placeholder to prevent flat_map from dropping empty lines
                    [b'\n'].chunks(1)
                } else {
                    cs.chunks(chunk_size)
                }
            })
            .chain(std::iter::repeat(&[b'~'][..]))
            .take(height)
            .enumerate()
            .for_each(|(line, bytes)| {
                write!(stdout, "{}", cursor::Goto(1, (line + 1) as _)).unwrap();

                let mut cut = 0;
                if let Some(matches) = self.search.as_ref().map(|regex| regex.find_iter(bytes)) {
                    matches.for_each(|m| {
                        stdout.write_all(&bytes[cut..m.start()]).unwrap();
                        write!(
                            stdout,
                            "{}{}",
                            color::Bg(color::Black),
                            color::Fg(color::LightWhite)
                        )
                        .unwrap();
                        stdout.write_all(&bytes[m.start()..m.end()]).unwrap();
                        write!(
                            stdout,
                            "{}{}",
                            color::Bg(color::Reset),
                            color::Fg(color::Reset)
                        )
                        .unwrap();
                        cut = m.end();
                    });
                }

                stdout.write_all(&bytes[cut..]).unwrap();
            });

        stdout.flush()?;

        Ok(())
    }

    fn draw_status(
        &self,
        stdout: &mut AlternateScreen<RawTerminal<std::io::Stdout>>,
        status: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        write!(
            stdout,
            "{}{}{}{}{}{}{}",
            cursor::Goto(1, self.size.1),
            clear::CurrentLine,
            color::Bg(color::Black),
            color::Fg(color::LightWhite),
            status,
            color::Bg(color::Reset),
            color::Fg(color::Reset),
        )?;
        stdout.flush()?;

        Ok(())
    }
}

enum ReadlineState {
    Initial,
    Slash(String),
    Number(i64),
}
enum Action {
    Status,
    Exit,
    JumpToTop,
    JumpToBottom,
    NextLine,
    PrevLine,
    Search(String),
    SearchNext,
    SearchPrev,
    Jump(i64),
}

use std::fmt;
impl fmt::Display for ReadlineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadlineState::Initial => write!(f, ":"),
            ReadlineState::Slash(s) => write!(f, "search:{}", s),
            ReadlineState::Number(i) => write!(f, "jump:{}", i),
        }
    }
}

impl ReadlineState {
    fn next(&mut self, key: Key) -> Option<Action> {
        match self {
            ReadlineState::Initial => match key {
                Key::Char('G') => Some(Action::JumpToBottom),
                Key::Char('N') => Some(Action::SearchPrev),
                Key::Char('g') => Some(Action::JumpToTop),
                Key::Char('j') => Some(Action::NextLine),
                Key::Char('k') => Some(Action::PrevLine),
                Key::Char('n') => Some(Action::SearchNext),
                Key::Char('q') => Some(Action::Exit),
                Key::Ctrl('g') => Some(Action::Status),

                Key::Down => Some(Action::NextLine),
                Key::Up => Some(Action::PrevLine),

                Key::Char('/') => {
                    *self = ReadlineState::Slash(String::new());
                    None
                }
                Key::Char(c) if c > '0' && c <= '9' => {
                    *self = ReadlineState::Number(c.to_digit(10).unwrap() as i64);
                    None
                }
                _ => None,
            },
            ReadlineState::Slash(s) => match key {
                Key::Esc => {
                    *self = ReadlineState::Initial;
                    None
                }
                Key::Backspace => {
                    s.pop();
                    if s.is_empty() {
                        *self = ReadlineState::Initial;
                        None
                    } else {
                        Some(Action::Search(s.clone()))
                    }
                }
                Key::Char('\n') => {
                    let search = s.clone();
                    *self = ReadlineState::Initial;
                    Some(Action::Search(search))
                }
                Key::Char(c) => {
                    s.push(c);

                    if s.chars().filter(|c| *c != '.').count() == 0 {
                        None // do not search this pattern
                    } else {
                        Some(Action::Search(s.clone()))
                    }
                }
                Key::Down => todo!(),
                Key::Up => todo!(),
                _ => None,
            },
            ReadlineState::Number(i) => match key {
                Key::Esc => {
                    *self = ReadlineState::Initial;
                    None
                }
                Key::Char('\n') => {
                    let jump = *i;
                    *self = ReadlineState::Initial;
                    Some(Action::Jump(jump))
                }
                Key::Char(c) if c > '0' && c <= '9' => {
                    let new = 10 * *i + c.to_digit(10).unwrap() as i64;
                    *self = ReadlineState::Number(new);
                    None
                }
                _ => None,
            },
        }
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

/// `less` implementation
fn less(filename: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(filename)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };
    let size = terminal_size()?;

    let mut pager = Pager::new(mmap, size);

    let mut stdout = AlternateScreen::from(stdout().into_raw_mode()?);
    write!(stdout, "{}", cursor::Hide)?;

    let stdin = stdin();
    let stdin = stdin.lock();

    let mut readline = ReadlineState::Initial;
    pager.draw_onto(&mut stdout)?;
    pager.draw_status(&mut stdout, filename)?;

    for c in stdin.keys() {
        let c = c?;
        let action = readline.next(c);
        if let Some(action) = action {
            let redraw = match action {
                Action::Exit => break,
                Action::Status => true, // todo
                Action::JumpToTop => pager.jump_to_top(),
                Action::JumpToBottom => pager.jump_to_bottom(),
                Action::NextLine => pager.scroll_down(),
                Action::PrevLine => pager.scroll_up(),
                Action::SearchNext => pager.search_next(),
                Action::SearchPrev => pager.search_prev(),
                Action::Search(s) => pager.search(&s),
                Action::Jump(_s) => true, // todo
            };
            if redraw {
                pager.draw_onto(&mut stdout)?;
            } else {
                emit_bell();
            }
        }
        pager.draw_status(&mut stdout, &format!("{}", readline))?;
    }

    write!(stdout, "{}", cursor::Show)?;

    Ok(())
}
