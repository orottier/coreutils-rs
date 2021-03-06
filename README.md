# coreutils-rs

Linux (core) utilities rewritten in Rust, for fun and profit

|         | done                                                   | todo                                                                                     | deps                         |
|---------|--------------------------------------------------------|------------------------------------------------------------------------------------------|------------------------------|
| cat     | output file<br>output stdin<br>line numbers            | some print options                                                                       |                              |
| cut     | cut bytes, chars, fields                               | multiple ranges<br>...                                                                   | clap                         |
| du      | count and summarize paths                              | do not visit paths twice<br>symlinks<br>all other options                                |                              |
| less    | show file<br>cursor navigation<br>search and highlight | show stdin<br>searching backwards<br>terminal resizing<br>page up/down<br>tailing<br>... | termion<br>regex<br>memmap   |
| ping    | ipv4<br>ipv6 (somewhat)<br>resolving                   | ipv6 sequence numbers<br>icmp identifiers<br>report ttl, damaged<br>...                  | pnet                         |
| pv      | stats<br>progress bar<br>                              | ...                                                                                      | indicatif                    |
| sort    | byte order<br>in-mem<br>external (batch)<br>parallel   | other ordering<br>other options                                                          | tempfile<br>clap<br>num\_cpus|
| sponge  | spong to file<br>sponge to stdout<br>append            | use tempfiles<br>atomic file mv                                                          |                              |
| tail    | tail file<br>tail stdin<br>follow -f<br>lines -n       | tail multiple files<br>...                                                               | clap                         |
| tee     | tee to file(s)<br>append                               | ignore SIGINT option                                                                     |                              |
| timeout | run cmd with time limit                                | send signals                                                                             |                              |
| wc      | parallel<br>fast path for line count                   | print summary per input<br>fast path for byte count                                      |                              |
| xargs   | batch<br>single<br>parallel<br>max args<br>verbose     | less unwraps()<br>more options                                                           | clap                         |

# Build instructions

For slim binaries:

```
RUSTFLAGS='-C link-arg=-s' cargo build --release
```

# Tests

no tests yet

# Docs

```
cargo doc --no-deps --open
```
