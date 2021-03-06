//! Input/Output helpers and abstractions

use std::convert::TryFrom;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;

#[derive(Debug)]
pub enum InputArg<S: AsRef<Path>> {
    /// Standard input
    Stdin,
    /// File(filename)
    File(S),
}

pub enum Input {
    Stdin(io::Stdin),
    File(File),
}

impl<S: AsRef<Path>> TryFrom<&InputArg<S>> for Input {
    type Error = io::Error;

    fn try_from(value: &InputArg<S>) -> Result<Self, Self::Error> {
        match value {
            InputArg::Stdin => Ok(Self::Stdin(io::stdin())),
            InputArg::File(filename) => File::open(filename).map(Self::File),
        }
    }
}

impl Input {
    pub fn into_read(self) -> Box<dyn Read + Send> {
        match self {
            Input::Stdin(stdin) => Box::new(stdin),
            Input::File(file) => Box::new(file),
        }
    }

    pub fn as_bufread(&self) -> impl BufRead + '_ {
        match &self {
            Input::Stdin(stdin) => Box::new(stdin.lock()) as Box<dyn BufRead>,
            Input::File(file) => Box::new(BufReader::new(file)) as _,
        }
    }

    pub fn into_bufread(self) -> impl BufRead {
        match self {
            Input::Stdin(stdin) => Box::new(BufReader::new(stdin)) as Box<dyn BufRead>,
            Input::File(file) => Box::new(BufReader::new(file)) as _,
        }
    }
}

pub enum OutputArg<S: AsRef<Path>> {
    /// Standard output
    Stdout,
    /// File(filename, append)
    File(S, bool),
}

pub enum Output {
    Stdout(io::Stdout),
    File(File),
}

impl<S: AsRef<Path>> TryFrom<&OutputArg<S>> for Output {
    type Error = io::Error;

    fn try_from(value: &OutputArg<S>) -> Result<Self, Self::Error> {
        match value {
            OutputArg::Stdout => Ok(Self::Stdout(io::stdout())),
            OutputArg::File(filename, append) => OpenOptions::new()
                .write(true)
                .create(true)
                .append(*append)
                .truncate(!append)
                .open(filename)
                .map(Self::File),
        }
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(buf),
            Self::File(file) => file.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            Self::File(file) => file.flush(),
        }
    }
}
