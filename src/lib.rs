use std::process::exit;

pub fn print_help_and_exit(usage: &str) -> ! {
    eprintln!("{}", usage);
    exit(1);
}
