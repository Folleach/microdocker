// This file contains routine helper functions; nothing interesting here.

use std::{env, error::Error, fmt::Display, fs::{self, DirBuilder}, io::Write, os::unix::fs::DirBuilderExt, path::PathBuf};

pub trait WithErrExt<T, E> where E : Display {
    fn with_err(self, prefix: &str) -> Result<T, String>;
}

impl<T, E> WithErrExt<T, E> for Result<T, E> where E : Display {
    fn with_err(self, prefix: &str) -> Result<T, String> {
        self.map_err(|x| format!("{prefix}: {x}"))
    }
}

// https://specifications.freedesktop.org/basedir-spec/latest/
pub fn get_microdocker_data_directory() -> Result<PathBuf, Box<dyn Error>> {
    let base = match env::var("XDG_DATA_HOME") {
        Ok(v) => PathBuf::new().join(v),
        Err(_) => match env::var("HOME") {
            Ok(v) => PathBuf::new().join(v).join(".local/share"),
            Err(e) => return Err(format!("failed to retrieve data directory: {}", e).into())
        }
    };

    create_if_not_exists(base.join("microdocker"), 0o700)
}

pub fn create_if_not_exists(path: PathBuf, mode: u32) -> Result<PathBuf, Box<dyn Error>> {
    DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(&path)
        .map_err(|e| std::io::Error::new(
            e.kind(),
            format!("failed to create directory {}: {e}", path.display()),
        ))?;
    Ok(path)
}

pub fn init_log(verbose: bool) {
    env_logger::Builder::new()
        .target(env_logger::Target::Stderr)
        .filter_level(if verbose { log::LevelFilter::Trace } else { log::LevelFilter::Info } )
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();
}
