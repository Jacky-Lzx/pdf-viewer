use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Gui {
        path: Option<PathBuf>,
        page: Option<usize>,
    },
    Info {
        path: PathBuf,
    },
    Text {
        path: PathBuf,
        page: usize,
    },
    Render {
        path: PathBuf,
        output: PathBuf,
        page: usize,
        width: u32,
    },
    Benchmark {
        path: PathBuf,
        iterations: usize,
    },
}

impl Command {
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Gui { path, .. } => path.as_deref(),
            Self::Info { path }
            | Self::Text { path, .. }
            | Self::Render { path, .. }
            | Self::Benchmark { path, .. } => Some(path),
            Self::Help | Self::Version => None,
        }
    }
}

pub fn parse(args: &[OsString]) -> Result<Command, String> {
    let Some(first) = args.first() else {
        return Ok(Command::Gui {
            path: None,
            page: None,
        });
    };
    let number = |index: usize, default: usize| -> Result<usize, String> {
        args.get(index).map_or(Ok(default), |value| {
            value
                .to_str()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| "Expected a positive number".to_owned())
        })
    };
    let page = |index| {
        number(index, 1)?
            .checked_sub(1)
            .ok_or_else(|| "Page numbers start at 1".to_owned())
    };
    match first.to_str() {
        Some("--help" | "-h") if args.len() == 1 => Ok(Command::Help),
        Some("--version" | "-V") if args.len() == 1 => Ok(Command::Version),
        Some("--info") => match &args[1..] {
            [] => Err("Missing PDF path. Run folio --help.".into()),
            [path] => Ok(Command::Info { path: path.into() }),
            _ => Err("Too many arguments for --info. Run folio --help.".into()),
        },
        Some("--page") => {
            if args.len() < 2 {
                return Err(
                    "Missing page number. Usage: folio --page PAGE [--] document.pdf".into(),
                );
            }
            let page = page(1)?;
            let path = match &args[2..] {
                [] => return Err("Missing PDF path. Run folio --help.".into()),
                [path] if path == "--" => return Err("Missing PDF path after --".into()),
                [path] => path,
                [separator, path] if separator == "--" => path,
                _ => return Err("Open one PDF at a time. Run folio --help.".into()),
            };
            Ok(Command::Gui {
                path: Some(path.into()),
                page: Some(page),
            })
        }
        Some("--render") => {
            if args.len() < 2 {
                return Err("Missing PDF path. Run folio --help.".into());
            }
            if args.len() < 3 {
                return Err("Missing output PNG path".into());
            }
            if args.len() > 5 {
                return Err("Too many arguments for --render. Run folio --help.".into());
            }
            let page = page(3)?;
            let width = number(4, 1200)?;
            if !(1..=16384).contains(&width) {
                return Err("Width must be between 1 and 16384".into());
            }
            Ok(Command::Render {
                path: args[1].clone().into(),
                output: args[2].clone().into(),
                page,
                width: width as u32,
            })
        }
        Some("--text" | "--benchmark") => {
            if args.len() < 2 {
                return Err("Missing PDF path. Run folio --help.".into());
            }
            if args.len() > 3 {
                return Err("Too many command arguments. Run folio --help.".into());
            }
            let path = args[1].clone().into();
            if first == "--text" {
                Ok(Command::Text {
                    path,
                    page: page(2)?,
                })
            } else {
                let iterations = number(2, 3)?;
                if !(1..=1000).contains(&iterations) {
                    return Err("Iterations must be between 1 and 1000".into());
                }
                Ok(Command::Benchmark { path, iterations })
            }
        }
        Some("--") => match &args[1..] {
            [] => Ok(Command::Gui {
                path: None,
                page: None,
            }),
            [path] => Ok(Command::Gui {
                path: Some(path.into()),
                page: None,
            }),
            _ => Err("Open one PDF at a time. Run folio --help.".into()),
        },
        _ if first.to_string_lossy().starts_with('-') => {
            Err("Unknown option. Run folio --help.".into())
        }
        _ if args.len() > 1 => Err("Open one PDF at a time. Run folio --help.".into()),
        _ => Ok(Command::Gui {
            path: Some(first.into()),
            page: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn validates_export_arguments_before_document_io() {
        for args in [
            vec!["--render"],
            vec!["--render", "missing.pdf"],
            vec!["--render", "missing.pdf", "out.png", "0"],
            vec!["--render", "missing.pdf", "out.png", "1", "0"],
            vec!["--render", "missing.pdf", "out.png", "1", "16385"],
            vec!["--render", "missing.pdf", "out.png", "1", "1200", "extra"],
            vec!["--text"],
            vec!["--info"],
            vec!["--info", "missing.pdf", "extra"],
            vec!["--text", "missing.pdf", "0"],
            vec!["--text", "missing.pdf", "-1"],
            vec!["--text", "missing.pdf", "1.5"],
            vec!["--benchmark", "missing.pdf", "0"],
            vec!["--benchmark", "missing.pdf", "1001"],
            vec!["--benchmark", "missing.pdf", "1", "extra"],
        ] {
            let error = command(&args).unwrap_err();
            assert!(!error.contains("Cannot read"), "{args:?}: {error}");
        }
        assert_eq!(
            command(&["--info", "in.pdf"]).unwrap(),
            Command::Info {
                path: "in.pdf".into(),
            }
        );
        assert_eq!(
            command(&["--render", "in.pdf", "out.png"]).unwrap(),
            Command::Render {
                path: "in.pdf".into(),
                output: "out.png".into(),
                page: 0,
                width: 1200,
            }
        );
        assert_eq!(
            command(&["--render", "in.pdf", "out.png", "42", "16384"]).unwrap(),
            Command::Render {
                path: "in.pdf".into(),
                output: "out.png".into(),
                page: 41,
                width: 16384,
            }
        );
        assert_eq!(
            command(&["--text", "in.pdf", "3"]).unwrap(),
            Command::Text {
                path: "in.pdf".into(),
                page: 2
            }
        );
    }

    #[test]
    fn gui_terminator_preserves_special_paths_and_rejects_extra_files() {
        assert_eq!(
            command(&[]).unwrap(),
            Command::Gui {
                path: None,
                page: None
            }
        );
        assert_eq!(
            command(&["--"]).unwrap(),
            Command::Gui {
                path: None,
                page: None
            }
        );
        for path in ["-notes.pdf", "--render", "--page", "文档 空格.pdf"] {
            assert_eq!(
                command(&["--", path]).unwrap(),
                Command::Gui {
                    path: Some(path.into()),
                    page: None
                }
            );
        }
        assert!(command(&["-notes.pdf"]).is_err());
        assert!(command(&["one.pdf", "two.pdf"]).is_err());
        assert!(command(&["--", "one.pdf", "two.pdf"]).is_err());
        assert_eq!(command(&["--version"]).unwrap(), Command::Version);
        assert_eq!(command(&["-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn gui_start_page_is_one_based_and_requires_one_unmodified_path() {
        for path in ["book.pdf", "-notes.pdf", "文档 空格.pdf"] {
            let expected = Command::Gui {
                path: Some(path.into()),
                page: Some(41),
            };
            assert_eq!(command(&["--page", "42", path]).unwrap(), expected);
            assert_eq!(command(&["--page", "42", "--", path]).unwrap(), expected);
        }
        for args in [
            vec!["--page"],
            vec!["--page", "42"],
            vec!["--page", "42", "--"],
            vec!["--page", "0", "book.pdf"],
            vec!["--page", "-1", "book.pdf"],
            vec!["--page", "1.5", "book.pdf"],
            vec!["--page", "99999999999999999999999999999999", "book.pdf"],
            vec!["--page", "1", "one.pdf", "two.pdf"],
            vec!["--page", "1", "--", "one.pdf", "two.pdf"],
        ] {
            assert!(command(&args).is_err(), "{args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_survive_parsing_and_non_utf8_numbers_are_rejected() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"document-\xff.pdf".to_vec());
        assert_eq!(
            parse(std::slice::from_ref(&path))
                .unwrap()
                .path()
                .unwrap()
                .as_os_str(),
            path
        );
        assert_eq!(
            parse(&["--text".into(), path.clone()])
                .unwrap()
                .path()
                .unwrap()
                .as_os_str(),
            path
        );
        assert_eq!(
            parse(&["--info".into(), path.clone()])
                .unwrap()
                .path()
                .unwrap()
                .as_os_str(),
            path
        );
        assert_eq!(
            parse(&["--page".into(), "2".into(), path.clone()]).unwrap(),
            Command::Gui {
                path: Some(path.clone().into()),
                page: Some(1)
            }
        );
        assert!(
            parse(&[
                "--page".into(),
                OsString::from_vec(vec![0xff]),
                path.clone()
            ])
            .is_err()
        );
        assert!(parse(&["--text".into(), path, OsString::from_vec(vec![0xff])]).is_err());
    }
}
