use nix;
use std::io;

pub fn error(err: nix::Error) -> io::Error {
    match err {
        nix::Error::Sys(val) => val.into(),
        _ => unreachable!(),
    }
}
