use std::io;
use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, BufMut, BytesMut};
use futures_util::ready;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

use crate::common::net::poll_read_buf;
use crate::debug_log;
use crate::proxy::vless::vless_option::VlessOption;
use crate::proxy::{Address, UdpRead, UdpWrite};
use crate::{impl_async_read, impl_async_write};

pub struct VlessStream<S> {
    stream: S,
    option: VlessOption,
    header_buffer: BytesMut,
    header_pos: usize,
    response_read: bool,
    read_buffer: BytesMut,
    read_zero: bool,
    temp_addon_len: usize,
}

impl<S> VlessStream<S> {
    fn construct_header(option: &VlessOption) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_u8(0);
        buf.put_slice(option.uuid.as_bytes());
        buf.put_u8(0);
        if option.is_udp {
            buf.put_u8(2);
        } else {
            buf.put_u8(1);
        }
        match &option.addr {
            Address::SocketAddress(sa) => {
                buf.put_u16(sa.port());
                match sa {
                    std::net::SocketAddr::V4(v4) => {
                        buf.put_u8(1);
                        buf.put_slice(&v4.ip().octets());
                    }
                    std::net::SocketAddr::V6(v6) => {
                        buf.put_u8(3);
                        for seg in &v6.ip().segments() {
                            buf.put_u16(*seg);
                        }
                    }
                }
            }
            Address::DomainNameAddress(domain, port) => {
                buf.put_u16(*port);
                buf.put_u8(2);
                buf.put_u8(domain.len() as u8);
                buf.put_slice(domain.as_bytes());
            }
        }
        buf
    }

    pub fn new(option: VlessOption, stream: S) -> Self {
        let header = Self::construct_header(&option);
        Self {
            stream,
            option,
            header_buffer: header,
            header_pos: 0,
            response_read: false,
            read_buffer: BytesMut::new(),
            read_zero: false,
            temp_addon_len: 0,
        }
    }
}

impl<S> VlessStream<S>
where
    S: AsyncRead + Unpin,
{
    fn priv_poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        dst: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.response_read {
            // Read VLESS response header: 2 bytes version + addon_len
            loop {
                if this.read_buffer.len() < 2 {
                    let n = ready!(poll_read_buf(&mut this.stream, cx, &mut this.read_buffer))?;
                    if n == 0 {
                        this.read_zero = true;
                        if this.read_buffer.is_empty() {
                            return Poll::Ready(Ok(()));
                        }
                        return Poll::Ready(Err(ErrorKind::UnexpectedEof.into()));
                    }
                    // If still not enough, return Pending to wait for more
                    if this.read_buffer.len() < 2 {
                        return Poll::Pending;
                    }
                }
                if this.read_buffer[0] != 0 {
                    debug_log!("vless: unexpected version {}", this.read_buffer[0]);
                    return Poll::Ready(Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!("unexpected vless version {}", this.read_buffer[0]),
                    )));
                }
                this.temp_addon_len = this.read_buffer[1] as usize;
                this.read_buffer.advance(2);
                if this.temp_addon_len > 0 {
                    while this.read_buffer.len() < this.temp_addon_len {
                        let n =
                            ready!(poll_read_buf(&mut this.stream, cx, &mut this.read_buffer))?;
                        if n == 0 {
                            this.read_zero = true;
                            return Poll::Ready(Err(ErrorKind::UnexpectedEof.into()));
                        }
                        if this.read_buffer.len() < this.temp_addon_len {
                            return Poll::Pending;
                        }
                    }
                    this.read_buffer.advance(this.temp_addon_len);
                }
                this.response_read = true;
                break;
            }
        }
        Pin::new(&mut this.stream).poll_read(cx, dst)
    }
}

impl<S> VlessStream<S>
where
    S: AsyncWrite + Unpin,
{
    fn priv_poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // Write header first
        while this.header_pos < this.header_buffer.len() {
            let n = ready!(Pin::new(&mut this.stream)
                .poll_write(cx, &this.header_buffer[this.header_pos..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "write zero byte into writer",
                )));
            }
            this.header_pos += n;
            if this.header_pos < this.header_buffer.len() {
                // Need more polling to finish header
                return Poll::Pending;
            }
        }
        // Header done, write payload
        let n = ready!(Pin::new(&mut this.stream).poll_write(cx, buf))?;
        if n == 0 && !buf.is_empty() {
            return Poll::Ready(Err(io::Error::new(
                ErrorKind::WriteZero,
                "write zero byte into writer",
            )));
        }
        Poll::Ready(Ok(n))
    }

    fn priv_poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn priv_poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

impl<S> AsyncRead for VlessStream<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.priv_poll_read(cx, buf)
    }
}

impl<S> AsyncWrite for VlessStream<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.priv_poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.priv_poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.priv_poll_shutdown(cx)
    }
}

impl<S: AsyncWrite + AsyncRead + Send + Unpin> UdpRead for VlessStream<S> {
    fn poll_recv_from(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<Address>> {
        let addr = self.option.addr.clone();
        let poll = self.priv_poll_read(cx, buf);
        match poll {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(addr)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S: AsyncWrite + AsyncRead + Send + Unpin> UdpWrite for VlessStream<S> {
    fn poll_send_to(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
        _target: &Address,
    ) -> Poll<io::Result<usize>> {
        self.priv_poll_write(cx, buf)
    }
}
