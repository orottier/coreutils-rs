//! Helpers for plowing through files
use std::io::{self, Read};

/// Iterate in large chunks over inputs
///
/// The inputs will be split up in blocks of approximately `max_chunk_size`.
/// Blocks are aligned on the given `delimiter` and will not include
/// the trailing delimiter.
///
/// This is an Iterator with `Item = io::Error<Vec<u8>>`
///
/// ```
/// use coreutils::chunks::ChunkedReader;
///
/// let input1 = b"hello
/// world
/// how
/// are
/// you?
/// ";
/// let input2 = b"good";
///
/// let inputs = vec![
///     Box::new(&input1[..]) as Box<dyn std::io::Read>,
///     Box::new(&input2[..]) as Box<dyn std::io::Read>,
/// ];
///
/// let mut reader = ChunkedReader::new(inputs, b'\n', 16);
///
/// assert_eq!(reader.next().unwrap().unwrap(), b"hello\nworld\nhow");
/// assert_eq!(reader.next().unwrap().unwrap(), b"are\nyou?");
/// assert_eq!(reader.next().unwrap().unwrap(), b"good");
/// assert!(reader.next().is_none());
/// ```
pub struct ChunkedReader<'a> {
    /// Collection of inputs
    inputs: Vec<Box<dyn Read + 'a>>,
    /// Chunks start/end with this delimiter
    delimiter: u8,
    /// Max chunk size, may be exceeded if no delimiters are found
    max_chunk_size: usize,
    /// Internal buffer for splitting
    next_buf: Vec<u8>,
}

impl<'a> ChunkedReader<'a> {
    /// Create a new chunked reader
    ///
    /// Arguments
    /// - inputs: Collection of inputs
    /// - delimiter:  Chunks start/end with this delimiter
    /// - max_chunk_size: may be exceeded if no delimiters are found
    ///
    /// Panics if max_chunk_size == 0
    pub fn new(inputs: Vec<Box<dyn Read + 'a>>, delimiter: u8, max_chunk_size: usize) -> Self {
        if max_chunk_size == 0 {
            panic!("max_chunk_size should be > 0");
        }

        Self {
            inputs,
            delimiter,
            max_chunk_size,
            next_buf: vec![],
        }
    }
}

impl Iterator for ChunkedReader<'_> {
    type Item = io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        // Get next input
        let input = match self.inputs.get_mut(0) {
            // all inputs exhausted, no leftover chunks
            None if self.next_buf.is_empty() => return None,
            // a leftover chunk
            None => {
                let mut buf = std::mem::replace(&mut self.next_buf, vec![]);
                if buf.last() == Some(&self.delimiter) {
                    buf.pop();
                }
                return Some(Ok(buf));
            }
            // input available
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
                // IO error
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
            // remove this input from the list
            self.inputs.remove(0);

            // shrink the allocated buffer
            buf.resize(pos, 0);

            // remove trailing delimiter, if present
            if buf.last() == Some(&self.delimiter) {
                buf.pop();
            }

            // TODO, maybe glue together with next input file?
            // in that case, make sure it is delimited properly
            return Some(Ok(buf));
        }

        // cut the buffer at the given delimiter
        let delim_pos = buf.iter().rposition(|b| *b == self.delimiter);
        match delim_pos {
            // no delimiter found in entire input, we need to read forward now
            None => todo!(),
            // delimiter found at pos
            Some(pos) => {
                // store this chunk for the next run
                self.next_buf = buf.split_off(pos + 1);
                buf.pop(); // remove the delimiter
            }
        }

        Some(Ok(buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_zero_chunk_size() {
        ChunkedReader::new(vec![], 0x0, 0);
    }

    #[test]
    fn test_empty() {
        let data = &[][..];
        let inputs = vec![Box::new(data) as Box<dyn Read>];
        let mut reader = ChunkedReader::new(inputs, 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), vec![]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_1_input_1_piece() {
        let data = &[1u8; 10][..];
        let inputs = vec![Box::new(data) as Box<dyn Read>];
        let mut reader = ChunkedReader::new(inputs, 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 10]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_1_input_2_piece() {
        let data = &[1u8, 1, 1, 0, 1, 1, 1, 0][..];
        let inputs = vec![Box::new(data) as Box<dyn Read>];
        let mut reader = ChunkedReader::new(inputs, 0x0, 6);

        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_only_delims() {
        let data = &[0u8; 10][..];
        let inputs = vec![Box::new(data) as Box<dyn Read>];
        let mut reader = ChunkedReader::new(inputs, 0x0, 6);

        assert_eq!(reader.next().unwrap().unwrap(), vec![0u8; 5]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![0u8; 3]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_delim_at_start() {
        let data = &[0u8, 1, 1, 1, 1, 1, 1, 1, 1, 1][..];
        let inputs = vec![Box::new(data) as Box<dyn Read>];
        let mut reader = ChunkedReader::new(inputs, 0x0, 10);

        assert_eq!(reader.next().unwrap().unwrap(), vec![]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1; 9]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_multiple_empty_inputs() {
        let data = &[][..];
        let inputs = vec![
            Box::new(data) as Box<dyn Read>,
            Box::new(data) as Box<dyn Read>,
        ];
        let mut reader = ChunkedReader::new(inputs, 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), vec![]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_2_input_1_piece() {
        let data = &[1u8; 10][..];
        let inputs = vec![
            Box::new(data) as Box<dyn Read>,
            Box::new(data) as Box<dyn Read>,
        ];
        let mut reader = ChunkedReader::new(inputs, 0x0, 256);

        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 10]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 10]);

        assert!(reader.next().is_none());
    }

    #[test]
    fn test_2_input_2_piece() {
        let data = &[1u8, 1, 1, 0, 1, 1, 1, 0][..];
        let inputs = vec![
            Box::new(data) as Box<dyn Read>,
            Box::new(data) as Box<dyn Read>,
        ];
        let mut reader = ChunkedReader::new(inputs, 0x0, 6);

        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);
        assert_eq!(reader.next().unwrap().unwrap(), vec![1u8; 3]);

        assert!(reader.next().is_none());
    }
}
