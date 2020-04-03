use std::convert::TryFrom;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::process::exit;

pub fn print_help_and_exit(usage: &str) -> ! {
    eprintln!("{}", usage);
    exit(1);
}

pub enum InputArg {
    /// Standard input
    Stdin,
    /// File(filename)
    File(String),
}

pub enum Input {
    Stdin(io::Stdin),
    File(File),
}

impl TryFrom<&InputArg> for Input {
    type Error = io::Error;

    fn try_from(value: &InputArg) -> Result<Self, Self::Error> {
        match value {
            InputArg::Stdin => Ok(Self::Stdin(io::stdin())),
            InputArg::File(filename) => File::open(filename).map(Self::File),
        }
    }
}

impl Input {
    pub fn as_bufread(&self) -> Box<dyn BufRead + '_> {
        match &self {
            Input::Stdin(stdin) => Box::new(stdin.lock()),
            Input::File(file) => Box::new(BufReader::new(file)),
        }
    }
}

pub enum OutputArg {
    /// Standard output
    Stdout,
    /// File(filename, append)
    File(String, bool),
}

pub enum Output {
    Stdout(io::Stdout),
    File(File),
}

impl TryFrom<&OutputArg> for Output {
    type Error = io::Error;

    fn try_from(value: &OutputArg) -> Result<Self, Self::Error> {
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
