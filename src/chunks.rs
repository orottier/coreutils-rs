//! Helpers for plowing through files
//!
//! The `ChunkedReader` can be used on your IO thread. It will
//! read and split your input in blocks of approximately `max_chunk_size`.  Blocks are aligned on
//! the given `delimiter` and will not include the trailing delimiter. You can let your thread pool
//! process the blocks so the IO thread can continue.
//!
//! The ChunkedReader is an Iterator with `Item = io::Error<ChunkedItem>`
//!
//! ```
//! use coreutils::chunks::{ChunkedReader, ChunkedItem};
//!
//! let input = b"hello
//! world
//! how
//! are
//! you?
//! ";
//!
//! // create a reader that clips on newlines, with max length 16
//! let mut reader = ChunkedReader::new(input.as_ref(), b'\n', 16);
//!
//! assert_eq!(reader.next().unwrap().unwrap(), ChunkedItem::Chunk(b"hello\nworld\nhow".to_vec()));
//! assert_eq!(reader.next().unwrap().unwrap(), ChunkedItem::Chunk(b"are\nyou?".to_vec()));
//! assert!(reader.next().is_none());
//! ```
//!
//! The iterator will bail when its internal buffer is full and no delimiter is found, to prevent
//! excessive memory usage. In this case the data read so far and the original Read input will be
//! returned:
//!
//! ```
//! use coreutils::chunks::{ChunkedReader, ChunkedItem};
//! use std::io::Read;
//!
//! let data = b"12345\n123456789012345\n".to_vec();
//!
//! // create a reader that clips on newlines, with max length 10
//! let mut reader = ChunkedReader::<&[u8]>::new(data.as_ref(), b'\n', 10);
//!
//! // first chunk will be succesfull
//! assert_eq!(reader.next().unwrap().unwrap(), ChunkedItem::Chunk(b"12345".to_vec()));
//!
//! // next chunk should bail, since it does not fit in the buffer
//! let (data, mut read) = match reader.next().expect("empty").expect("io err") {
//!     ChunkedItem::Chunk(_data) => unreachable!(),
//!     ChunkedItem::Bail(data, read) => (data, read),
//! };
//!
//! // ten bytes of data were consumed
//! assert_eq!(data, b"1234567890".to_vec());
//!
//! // 6 bytes are left in the reader
//! let mut left = vec![];
//! assert_eq!(read.read_to_end(&mut left).unwrap(), 6);
//! assert_eq!(left, b"12345\n".to_vec());
//!
//! // reader is exhausted
//! assert!(reader.next().is_none());
//! ```

use std::io::{self, Read};

/// Iterate in large chunks over input
pub struct ChunkedReader<R> {
    /// input (dyn Read)
    input: Option<R>,
    /// Chunks start/end with this delimiter
    delimiter: u8,
    /// Max chunk size, may be exceeded if no delimiters are found
    max_chunk_size: usize,
    /// Internal buffer for splitting
    next_buf: Vec<u8>,
}

impl<R: Read> ChunkedReader<R> {
    /// Create a new chunked reader
    ///
    /// Arguments
    /// - input: dyn Read
    /// - delimiter:  Chunks start/end with this delimiter
    /// - max_chunk_size: may be exceeded if no delimiters are found
    ///
    /// Panics if max_chunk_size == 0
    pub fn new(input: R, delimiter: u8, max_chunk_size: usize) -> Self {
        if max_chunk_size == 0 {
            panic!("max_chunk_size should be > 0");
        }

        Self {
            input: Some(input),
            delimiter,
            max_chunk_size,
            next_buf: vec![],
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ChunkedItem<R> {
    Chunk(Vec<u8>),
    Bail(Vec<u8>, R),
}

impl<R: Read + Send> Iterator for ChunkedReader<R> {
    type Item = io::Result<ChunkedItem<R>>;

    fn next(&mut self) -> Option<Self::Item> {
        // for convencience, put the input attribute in a local var
        let mut input = match self.input.take() {
            None => return None,
            Some(input) => input,
        };

        // There may be leftover data from previous call.
        // Take it and make sure our buffer has the correct capacity
        let mut buf = std::mem::replace(&mut self.next_buf, vec![]);
        let mut pos = buf.len();
        buf.resize(self.max_chunk_size, 0);

        // Keep reading until the buffer is full
        let exhausted = loop {
            match input.read(&mut buf[pos..]) {
                // IO error, no further input will be possible
                Err(e) => return Some(Err(e)),
                // EOF for this input
                Ok(0) => break true,
                // successful read
                Ok(n) => pos += n,
            }

            // buffer full?
            if pos == self.max_chunk_size {
                break false;
            }
        };

        if exhausted {
            // shrink the allocated buffer
            buf.resize(pos, 0);

            // remove trailing delimiter, if present
            if buf.last() == Some(&self.delimiter) {
                buf.pop();
            }

            return Some(Ok(ChunkedItem::Chunk(buf)));
        }

        // cut the buffer at the given delimiter
        let delim_pos = buf.iter().rposition(|b| *b == self.delimiter);
        match delim_pos {
            // no delimiter found in entire input, bail out and let the caller handle this
            None => Some(Ok(ChunkedItem::Bail(buf, input))),

            // delimiter found at pos
            Some(pos) => {
                // store this chunk for the next run
                self.next_buf = buf.split_off(pos + 1);
                buf.pop(); // remove the delimiter

                // put back our input attribute
                self.input = Some(input);

                Some(Ok(ChunkedItem::Chunk(buf)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ChunkedItem::*;

    #[test]
    #[should_panic]
    fn test_zero_chunk_size() {
        let data = [1u8; 10];
        ChunkedReader::new(data.as_ref(), 0x0, 0);
    }

    #[test]
    fn test_empty() {
        let data = [];
        let mut reader = ChunkedReader::new(data.as_ref(), 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![]));

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_1_piece() {
        let data = [1u8; 10];
        let mut reader = ChunkedReader::new(data.as_ref(), 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![1u8; 10]));

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_2_piece() {
        let data = [1u8, 1, 1, 0, 1, 1, 1, 0];
        let mut reader = ChunkedReader::new(data.as_ref(), 0x0, 6);

        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![1u8; 3]));
        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![1u8; 3]));

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_only_delims() {
        let data = [0u8; 10];
        let mut reader = ChunkedReader::new(data.as_ref(), 0x0, 6);

        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![0u8; 5]));
        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![0u8; 3]));

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_delim_at_start() {
        let data = [0u8, 1, 1, 1, 1, 1, 1, 1, 1, 1];
        let mut reader = ChunkedReader::new(data.as_ref(), 0x0, 10);

        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![]));
        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![1; 9]));

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_bail() {
        // 5 times b1, 1 time b0, 15 times b1
        let mut data = vec![1; 5];
        data.push(0);
        data.extend_from_slice(&[1; 15]);
        data.push(0);

        // create a reader that clips on b0, with max length 10
        let mut reader = ChunkedReader::<&[u8]>::new(data.as_ref(), 0x0, 10);

        // first chunk will be succesfull
        assert_eq!(reader.next().unwrap().unwrap(), Chunk(vec![1; 5]));

        // next chunk should bail, since it does not fit in the buffer
        let (data, mut read) = match reader.next().expect("empty").expect("io err") {
            Chunk(_data) => unreachable!(),
            Bail(data, read) => (data, read),
        };

        // ten bytes of data were consumed
        assert_eq!(data, vec![1; 10]);

        // 6 bytes are left in the reader
        let mut left = vec![];
        assert_eq!(read.read_to_end(&mut left).unwrap(), 6);
        assert_eq!(left, vec![1, 1, 1, 1, 1, 0]);

        // reader is exhausted
        assert!(reader.next().is_none());
    }
}
