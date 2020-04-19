//! Parallelism helpers

use std::num::NonZeroU32;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Runnable trait, side-effect only (no return val)
pub trait Job {
    fn run(self);
}

/// Executor for running Jobs
pub enum Executor<J> {
    /// single-threaded, zero overhead
    Synchronous,
    /// multi-threaded
    Concurrent(ThreadPool<J>),
}

/// Threadpool implementation for parallel execution
///
/// Job submission is handled with a `SyncSender` of size `n_threads`, which
/// means your thread calling `submit` will block if all workers are busy.
pub struct ThreadPool<J> {
    /// worker threads
    threads: Vec<JoinHandle<()>>,
    /// job submission channel
    sender: SyncSender<J>,
}

impl<J: Job + Send + 'static> ThreadPool<J> {
    /// create a new thread pool with given size
    pub fn new(size: NonZeroU32) -> Self {
        let size = size.get();
        let (sender, receiver) = sync_channel::<J>(size as _);
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

    /// submit a new job to the queue, this function will block if all workers are busy
    pub fn submit(&mut self, job: J) {
        self.sender.send(job).unwrap()
    }

    /// wait for all submitted jobs to finish, this shuts down the thread pool
    pub fn finish(self) {
        let ThreadPool {
            threads, sender, ..
        } = self;

        drop(sender); // signal receivers they can exit

        threads
            .into_iter()
            .for_each(|thread| thread.join().unwrap())
    }
}

impl<J: Job + Send + 'static> Executor<J> {
    /// submit a new job to the executor
    pub fn submit(&mut self, job: J) {
        match self {
            Executor::Synchronous => job.run(), // run immediately
            Executor::Concurrent(thread_pool) => thread_pool.submit(job),
        }
    }

    /// wait for all submitted jobs to finish
    pub fn finish(self) {
        match self {
            Executor::Synchronous => (),
            Executor::Concurrent(thread_pool) => thread_pool.finish(),
        }
    }
}
