use bytes::{BufMut, BytesMut};
use log::trace;
use std::io::{Error as IoError, ErrorKind};
use tokio_codec::{Decoder, Encoder};

use futures::prelude::{Async, AsyncSink, Poll, Sink, StartSend, Stream};
use futures::try_ready;

use super::{AsRawFd, UnixPacket};

#[derive(Debug)]
pub struct UnixPacketFramed<C> {
    socket: UnixPacket,
    codec: C,
    read: BytesMut,
    write: BytesMut,
    flushed: bool,
}

impl<C> UnixPacketFramed<C> {
    pub fn new(socket: UnixPacket, codec: C) -> UnixPacketFramed<C> {
        UnixPacketFramed {
            socket,
            codec,
            read: BytesMut::with_capacity(4096),
            write: BytesMut::with_capacity(4096),
            flushed: true,
        }
    }
}

impl<C: Decoder> Stream for UnixPacketFramed<C> {
    type Item = C::Item;
    type Error = C::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.read.reserve(4096);
        let received: usize = unsafe {
            let res = try_ready!(self.socket.poll_recv(self.read.bytes_mut()));
            self.read.set_len(res);
            res
        };
        trace!(
            "stream fd={:?} received {:?} bytes",
            self.socket.as_raw_fd(),
            received
        );
        let res = self.codec.decode(&mut self.read);
        self.read.clear();

        Ok(Async::Ready(res?))
    }
}

impl<C: Encoder> Sink for UnixPacketFramed<C> {
    type SinkItem = C::Item;
    type SinkError = C::Error;

    fn start_send(
        &mut self,
        item: Self::SinkItem,
    ) -> StartSend<Self::SinkItem, Self::SinkError> {
        if !self.flushed {
            match self.poll_complete()? {
                Async::Ready(()) => {}
                Async::NotReady => return Ok(AsyncSink::NotReady(item)),
            };
        }
        self.codec.encode(item, &mut self.write)?;
        self.flushed = false;
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), C::Error> {
        if self.flushed {
            return Ok(Async::Ready(()));
        }

        let sent = try_ready!({
            trace!(
                "sink fd={:?} send data {:?}",
                self.socket.as_raw_fd(),
                self.write
            );
            let res = self.socket.poll_send(&self.write);
            trace!(
                "sink fd={:?} send result {:?}",
                self.socket.as_raw_fd(),
                res
            );
            res
        });

        trace!(
            "sink fd={:?} sent {:?} bytes",
            self.socket.as_raw_fd(),
            sent
        );

        let wrote_all = sent == self.write.len();
        self.write.clear();
        self.flushed = true;

        if wrote_all {
            Ok(Async::Ready(()))
        } else {
            Err(IoError::new(
                ErrorKind::Other,
                "failed to write entire message to socket",
            )
            .into())
        }
    }

    fn close(&mut self) -> Poll<(), C::Error> {
        self.poll_complete()
    }
}
