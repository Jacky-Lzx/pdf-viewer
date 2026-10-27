use std::{
    fmt::Arguments,
    io::{self, Write},
};

pub fn line(arguments: Arguments<'_>) -> Result<(), String> {
    write_line(&mut io::stdout().lock(), arguments)
        .map_err(|error| format!("Cannot write standard output: {error}"))
}

fn write_line(output: &mut impl Write, arguments: Arguments<'_>) -> io::Result<()> {
    match writeln!(output, "{arguments}").and_then(|_| output.flush()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_formatting_and_newlines() {
        let mut output = Vec::new();
        write_line(&mut output, format_args!("中文 {}\nsecond", 42)).unwrap();
        assert_eq!(output, "中文 42\nsecond\n".as_bytes());
    }

    #[test]
    fn closed_pipes_are_normal_but_real_write_and_flush_errors_propagate() {
        struct Failed {
            kind: io::ErrorKind,
            on_flush: bool,
        }
        impl Write for Failed {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.on_flush {
                    Ok(bytes.len())
                } else {
                    Err(self.kind.into())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(self.kind.into())
            }
        }
        for on_flush in [false, true] {
            for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::PermissionDenied] {
                let result = write_line(&mut Failed { kind, on_flush }, format_args!("text"));
                if kind == io::ErrorKind::BrokenPipe {
                    result.unwrap();
                } else {
                    assert_eq!(result.unwrap_err().kind(), kind);
                }
            }
        }
    }
}
