use std::io::Error as IoError;

use tokio::runtime::Builder;
pub(crate) use tokio::runtime::Runtime;
pub(crate) use tokio::spawn;

pub(crate) fn new() -> Result<Runtime, IoError> {
    Builder::new().basic_scheduler().enable_io().build()
}
