use tokio::io;
use tokio::prelude::*;
use tokio::io::copy;
use tokio::io::Error;
use tokio::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use futures::*;
use bytes::BufMut;

use std::str::from_utf8;
use std::mem;



pub struct ReadAtMost<A, T> {
    state: State<A, T>,
}

enum State<A, T> {
    Reading {
        stream: A,
        buf: T,
        pos: usize,
    },
    Empty,
}

impl<A, T> Future for ReadAtMost<A, T>
where
    A: AsyncRead,
    T: AsMut<[u8]>,
{
    type Item = (A, T, usize);
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, io::Error> {
        match self.state {
            State::Reading {
                ref mut stream,
                ref mut buf,
                ref mut pos
            } => {
                let buf = buf.as_mut();
                while *pos < buf.len() {
                    let n = try_ready!({
                        stream.poll_read(&mut buf[*pos..])
                    });
                    *pos += n;
                    if n == 0 {
                        match mem::replace(&mut self.state, State::Empty) {
                            State::Reading { stream, buf, pos } => return Ok((stream, buf, pos).into()),
                            State::Empty => panic!(),
                        };
                    }
                }
            }
            State::Empty => panic!("poll a ReadAtMost after it's done"),
        }

        match mem::replace(&mut self.state, State::Empty) {
            State::Reading { stream, buf, pos } => {
                Ok(Async::Ready((stream, buf, pos)))
            }
            State::Empty => panic!(),
        }
    }
}

pub fn read_at_most<A, T>(stream: A, buf: T) -> ReadAtMost<A, T>
where
    A: AsyncRead,
    T: AsMut<[u8]>,
{
    ReadAtMost {
        state: State::Reading {
            stream: stream,
            buf: buf,
            pos: 0,
        },
    }
}
