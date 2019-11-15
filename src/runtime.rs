use std::io::Error as IoError;

use tokio::runtime::Builder;
pub(crate) use tokio::runtime::{spawn, Runtime};

pub(crate) fn new() -> Result<Runtime, IoError> {
    Builder::new().current_thread().build()
}
