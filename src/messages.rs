use crate::debug::bytes;
use bincode;
use bitflags::bitflags;
use log::trace;
use serde::de::DeserializeOwned;
use serde::{self, Deserialize, Serialize};
use std::io::{Error as IoError, ErrorKind, Write};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct StartMode : u32 {
        const PROCESS_GROUP = 1;
        const SESSION = 2;
        const DETACH_TERMINAL = 4;
        const NOHUP = 8;
    }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct Files : u32 {
        const IN = 1;
        const OUT = 2;
        const ERR = 4;
    }
}

#[derive(Debug, Clone)]
pub struct ProcessRequest<'a> {
    pub program: &'a str,
    pub argv: &'a [&'a str],
    pub cwd: &'a str,
    pub env: &'a [(&'a str, &'a str)],
    pub startup: StartMode,
    pub io: Files,
    pub pgid: i32,
    pub uid: i32,
    pub gid: i32,
    pub deathsig: i32,
}

impl<'a> From<&ExecRequestInput<'a>> for ProcessRequest<'a> {
    fn from(o: &ExecRequestInput<'a>) -> ProcessRequest<'a> {
        ProcessRequest {
            program: o.program,
            argv: o.argv,
            cwd: o.cwd,
            env: o.env,
            startup: o.startup,
            io: o.io,
            pgid: o.pgid,
            uid: o.uid,
            gid: o.gid,
            deathsig: o.deathsig,
        }
    }
}

impl<'a> From<&'a ExecRequestOutput<'a>> for ProcessRequest<'a> {
    fn from(o: &'a ExecRequestOutput<'a>) -> ProcessRequest<'a> {
        ProcessRequest {
            program: o.program,
            argv: o.argv.as_slice(),
            cwd: o.cwd,
            env: o.env.as_slice(),
            startup: o.startup,
            io: o.io,
            pgid: o.pgid,
            uid: o.uid,
            gid: o.gid,
            deathsig: o.deathsig,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExecHeader {
    pub body_size: usize,
}

#[derive(Serialize)]
pub enum RequestInput {
    Stop,
    Exec(ExecHeader),
}

#[derive(Deserialize)]
pub enum RequestOutput {
    Stop,
    Exec(ExecHeader),
}

#[derive(Serialize, Clone)]
pub struct ExecRequestInput<'a> {
    pub program: &'a str,
    pub argv: &'a [&'a str],
    pub cwd: &'a str,
    pub env: &'a [(&'a str, &'a str)],
    pub startup: StartMode,
    pub io: Files,
    pub pgid: i32,
    pub uid: i32,
    pub gid: i32,
    pub deathsig: i32,
    pub connsig: i32,
}

#[derive(Deserialize, Clone)]
pub struct ExecRequestOutput<'a> {
    pub program: &'a str,
    pub argv: Vec<&'a str>,
    pub cwd: &'a str,
    pub env: Vec<(&'a str, &'a str)>,
    pub startup: StartMode,
    pub io: Files,
    pub pgid: i32,
    pub uid: i32,
    pub gid: i32,
    pub deathsig: i32,
    pub connsig: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StartedProcess<'a> {
    pub success: bool,
    pub message: &'a str,
    pub errno: i32,
    pub pid: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Signal(pub i32);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ProcessResult {
    Undefined,
    Exit(i32),
    Signal(i32),
}

fn encoding_error(base: bincode::Error) -> IoError {
    use bincode::ErrorKind::*;
    match *base {
        Io(err) => err,
        error => IoError::new(ErrorKind::InvalidData, error),
    }
}

pub fn encode_request<W, T>(mut dest: W, req: &T) -> Result<(), IoError>
where
    W: Write + AsRef<[u8]>,
    T: Serialize,
{
    let result = bincode::serialize_into(&mut dest, &req);
    trace!("message encoding {:?}", bytes(&dest));
    result.map_err(encoding_error)
}

pub fn decode_request<B, T>(data: B) -> Result<T, IoError>
where
    B: AsRef<[u8]>,
    T: DeserializeOwned,
{
    trace!("message decoding {:?}", bytes(&data));
    bincode::deserialize_from(data.as_ref()).map_err(encoding_error)
}

pub fn decode_request_ref<'de, T>(data: &'de [u8]) -> Result<T, IoError>
where
    T: Deserialize<'de>,
{
    trace!("message decoding {:?}", bytes(&data));
    bincode::deserialize(data).map_err(encoding_error)
}
