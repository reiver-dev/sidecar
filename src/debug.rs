use std::fmt::Debug;

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

pub struct OptionDebug<'a, T: Debug>(pub &'a Option<T>);

pub fn opt<'a, T: Debug>(val: &'a Option<T>) -> OptionDebug<'a, T> {
    OptionDebug(val)
}

impl<'a, T: Debug> std::fmt::Debug for OptionDebug<'a, T> {
    fn fmt(
        &self,
        fmt: &mut std::fmt::Formatter,
    ) -> Result<(), std::fmt::Error> {
        match self.0 {
            Some(val) => val.fmt(fmt),
            None => Ok(()),
        }
    }
}
