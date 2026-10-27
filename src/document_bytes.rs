use std::io::{self, Read};

const BLOCK: u64 = 1024 * 1024;

/// Keep filesystem reads bounded without copying through an intermediate buffer.
pub fn read(
    reader: &mut impl Read,
    expected: u64,
    cancelled: impl Fn() -> bool,
) -> io::Result<Option<Vec<u8>>> {
    if cancelled() {
        return Ok(None);
    }
    let length = usize::try_from(expected)
        .map_err(|_| io::Error::other("File is too large for this address space"))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(io::Error::other)?;
    loop {
        let count = reader.by_ref().take(BLOCK).read_to_end(&mut bytes)?;
        if cancelled() {
            return Ok(None);
        }
        if count < BLOCK as usize {
            return Ok(Some(bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io::Cursor};

    #[test]
    fn cancellation_before_read_and_between_blocks_leaves_the_rest_unread() {
        let source = vec![37; BLOCK as usize * 3 + 19];
        let mut reader = Cursor::new(&source);
        assert!(
            read(&mut reader, source.len() as u64, || true)
                .unwrap()
                .is_none()
        );
        assert_eq!(reader.position(), 0);
        let checks = Cell::new(0);
        assert!(
            read(&mut reader, source.len() as u64, || {
                checks.set(checks.get() + 1);
                checks.get() == 2
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(reader.position(), BLOCK);
        let remaining = read(&mut reader, 0, || false).unwrap().unwrap();
        assert_eq!(remaining, source[BLOCK as usize..]);
    }

    #[test]
    fn complete_reads_match_the_source_for_empty_short_exact_and_multiple_blocks() {
        for length in [0, 13, BLOCK as usize, BLOCK as usize * 2 + 11] {
            let source: Vec<_> = (0..length).map(|index| (index % 251) as u8).collect();
            for expected in [0, length as u64 / 2, length as u64, length as u64 + 16] {
                assert_eq!(
                    read(&mut Cursor::new(&source), expected, || false)
                        .unwrap()
                        .unwrap(),
                    source
                );
            }
        }
    }

    #[test]
    fn interrupted_reads_retry_and_real_io_errors_propagate() {
        struct Interrupted {
            calls: usize,
        }
        impl Read for Interrupted {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                match self.calls {
                    1 => Err(io::ErrorKind::Interrupted.into()),
                    2 => {
                        output[..3].copy_from_slice(b"PDF");
                        Ok(3)
                    }
                    _ => Ok(0),
                }
            }
        }
        assert_eq!(
            read(&mut Interrupted { calls: 0 }, 0, || false)
                .unwrap()
                .unwrap(),
            b"PDF"
        );
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        assert_eq!(
            read(&mut Failed, 0, || false).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
