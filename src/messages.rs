use bytes::{BufMut, BytesMut};
use log::trace;
use rmp_serde;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use serde_derive::{Deserialize, Serialize};
use std::fmt::Debug;
use std::io::{Error as IoError, Write};
use std::marker::PhantomData;
use tokio_codec::{Decoder, Encoder};

#[derive(Serialize, Deserialize, Debug)]
pub struct Io {
    pub stdin: i32,
    pub stdout: i32,
    pub stderr: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
    pub io: Option<Io>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StartedProcess {
    pub success: bool,
    pub message: Option<String>,
    pub errno: i32,
    pub pid: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Signal(pub i32);

#[derive(Serialize, Deserialize, Debug)]
pub struct RetCode(pub i32);

pub fn encode_request<W, T>(dest: &mut W, req: &T)
where
    W: Write,
    T: Serialize,
{
    rmp_serde::encode::write(dest, &req).expect("failed to serialize request");
}

pub fn decode_request<B, T>(data: B) -> T
where
    B: AsRef<[u8]> + Debug,
    T: DeserializeOwned,
{
    trace!("decoding {:?}", bytes(&data));
    rmp_serde::from_slice(data.as_ref()).expect("invalid request")
}

pub struct Codec<R, W>(PhantomData<R>, PhantomData<W>);

impl<R, W> Codec<R, W> {
    pub fn new() -> Codec<R, W> {
        Codec(PhantomData, PhantomData)
    }
}

impl<R, W> Encoder for Codec<R, W>
where
    R: DeserializeOwned + Debug,
    W: Serialize + Debug,
{
    type Item = W;
    type Error = IoError;

    fn encode(
        &mut self,
        item: Self::Item,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        trace!("message encode {:?}", item);
        encode_request(&mut dst.writer(), &item);
        Ok(())
    }
}

impl<R, W> Decoder for Codec<R, W>
where
    R: DeserializeOwned + Debug,
    W: Serialize + Debug,
{
    type Item = R;
    type Error = IoError;

    fn decode(
        &mut self,
        src: &mut BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        trace!("message decode {:?}", src);
        if !src.is_empty() {
            Ok(Some(decode_request(src)))
        } else {
            Ok(None)
        }
    }
}

pub fn bytes<'a, T: AsRef<[u8]> + 'a>(val: &'a T) -> BytesDebug<'a> {
    BytesDebug(val.as_ref())
}

pub struct BytesDebug<'a>(pub &'a [u8]);

impl<'a> std::fmt::Debug for BytesDebug<'a> {
    fn fmt(
        &self,
        fmt: &mut std::fmt::Formatter,
    ) -> Result<(), std::fmt::Error> {
        write!(fmt, "b\"")?;
        for &c in self.0 {
            // https://doc.rust-lang.org/reference.html#byte-escapes
            if c == b'\n' {
                write!(fmt, "\\n")?;
            } else if c == b'\r' {
                write!(fmt, "\\r")?;
            } else if c == b'\t' {
                write!(fmt, "\\t")?;
            } else if c == b'\\' || c == b'"' {
                write!(fmt, "\\{}", c as char)?;
            } else if c == b'\0' {
                write!(fmt, "\\0")?;
            // ASCII printable
            } else if c >= 0x20 && c < 0x7f {
                write!(fmt, "{}", c as char)?;
            } else {
                write!(fmt, "\\x{:02x}", c)?;
            }
        }
        write!(fmt, "\"")?;
        Ok(())
    }
}
