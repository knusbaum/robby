use std::mem;

use futures::*;
use regex::bytes::Regex;
use tokio::{io, prelude::*};

pub struct ReadHttpHeader<A, T> {
    state: State<A, T>,
}

enum State<A, T> {
    Reading {
        stream: A,
        buf: T,
        pos: usize,
        split: usize,
    },
    Empty,
}

fn eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "early eof")
}

fn too_big() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "HTTP header exceeded max length",
    )
}

impl<A, T> Future for ReadHttpHeader<A, T>
where
    A: AsyncRead,
    T: AsMut<[u8]>,
{
    type Item = (A, T, usize, usize);
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, io::Error> {
        match self.state {
            State::Reading {
                ref mut stream,
                ref mut buf,
                ref mut pos,
                ref mut split,
            } => {
                let buf = buf.as_mut();
                while *pos < buf.len() {
                    let n = try_ready!({ stream.poll_read(&mut buf[*pos..]) });

                    //let re = Regex::new(r"\r\n\r\n").unwrap();
                    lazy_static! {
                        static ref RE: Regex = Regex::new(r"\r\n\r\n").unwrap();
                    }
                    // We want to backup enough to find the end sequence in case
                    // part of the end sequence came in previously.
                    let mut backup = *pos;
                    if backup < 3 {
                        backup = 0;
                    } else {
                        backup = backup - 3;
                    }

                    *pos += n;

                    let caps = RE.captures(&buf[backup..*pos]);
                    if let Some(captures) = caps {
                        let end_match = captures.get(0).unwrap();
                        *split = backup + end_match.end();
                        break;
                    }

                    if n == 0 {
                        return Err(eof());
                    }
                }
                if *pos == buf.len() {
                    return Err(too_big());
                }
            }
            State::Empty => panic!("poll a ReadHttpHeader after it's done"),
        }

        match mem::replace(&mut self.state, State::Empty) {
            State::Reading {
                stream,
                buf,
                pos,
                split,
            } => Ok(Async::Ready((stream, buf, pos, split))),
            State::Empty => panic!(),
        }
    }
}

pub fn read_http_header<A, T>(stream: A, buf: T) -> ReadHttpHeader<A, T>
where
    A: AsyncRead,
    T: AsMut<[u8]>,
{
    ReadHttpHeader {
        state: State::Reading {
            stream: stream,
            buf: buf,
            pos: 0,
            split: 0,
        },
    }
}
