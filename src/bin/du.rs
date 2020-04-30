//! `du`: estimate file space usage
//!
//! Special effort has been put in a non-recursive program flow,
//! instead we use a linked list of actions with a 'total size' carry.
//!
//! Todo:
//! - do not visit paths twice
//! - apparent or actual block size?
//! - implement all options

use std::fs::Metadata;
use std::path::PathBuf;
use std::process::exit;

use coreutils::util::print_help_and_exit;

const USAGE: &str = "du [OPTION]... [FILE]...: estimate file space usage";

#[cfg(target_os = "linux")]
#[inline(always)]
pub fn file_size(attr: &Metadata) -> u64 {
    use std::os::linux::fs::MetadataExt;
    attr.st_blocks()
}

#[cfg(target_os = "macos")]
#[inline(always)]
pub fn file_size(attr: &Metadata) -> u64 {
    use std::os::macos::fs::MetadataExt;
    attr.st_blocks()
}

#[cfg(target_os = "windows")]
#[inline(always)]
pub fn file_size(attr: &Metadata) -> u64 {
    attr.len()
}

/// Parse arguments, run job, pass return code
fn main() -> ! {
    let mut args = std::env::args();
    args.next(); // bin name

    let mut inputs: Vec<String> = args
        .map(|arg| {
            if arg.starts_with('-') {
                print_help_and_exit(USAGE)
            } else {
                arg
            }
        })
        .collect();

    if inputs.is_empty() {
        inputs.push(String::from("."));
    }

    du(inputs);

    exit(0)
}

/// Task instruction to enter or summarize given path
///
/// This program implements `du` in a non-recursive way,
/// instead a linked list of `DuTask`s is used.
struct DuTask {
    /// task to run up next
    next: Option<Box<DuTask>>,
    /// enter or summarize this path
    path: PathBuf,
    /// when true, enter this path and count files, otherwise print summary
    enter: bool,
}

impl DuTask {
    pub fn new(path: String) -> Self {
        Self {
            next: None,
            path: PathBuf::from(path),
            enter: true,
        }
    }

    pub fn run(self, carry: &mut Vec<u64>) -> Option<Box<Self>> {
        let DuTask { next, path, enter } = self;

        if enter {
            let entries_result = path.read_dir();

            let summary_task = Self {
                next,
                path,
                enter: false,
            };

            let mut next_holder = Some(Box::new(summary_task));
            let mut size = 0;

            if let Ok(entries) = entries_result {
                for entry_result in entries {
                    if let Ok(entry) = entry_result {
                        let attr = entry.metadata().unwrap();

                        if attr.is_dir() {
                            let task = Self {
                                next: next_holder.take(),
                                path: entry.path(),
                                enter: true,
                            };
                            next_holder = Some(Box::new(task));
                        } else {
                            size += file_size(&attr);
                        }
                    } else {
                        eprintln!("{:?}", entry_result.unwrap_err());
                    }
                }
            } else {
                eprintln!("{:?}", entries_result.unwrap_err());
            }

            carry.push(size);

            next_holder.take()
        } else {
            let total_size = carry.pop().unwrap();
            println!("{:<10} {}", total_size, path.to_string_lossy());

            let prev_index = carry.len() - 1;
            carry[prev_index] += total_size;

            next
        }
    }
}

/// `du` implementation
fn du(mut inputs: Vec<String>) {
    // iterate all input paths
    while let Some(input) = inputs.pop() {
        let mut task_holder = Some(Box::new(DuTask::new(input)));

        // This is the stack of directory sizes, the index corresponds with the depth.
        // Start with the root dir, of size zero.
        let mut carry = vec![0];

        while let Some(task) = task_holder.take() {
            task_holder = task.run(&mut carry);
        }
    }
}
