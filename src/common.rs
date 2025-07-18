// This file contains routine helper functions; nothing interesting here.

use std::{env, error::Error, fmt::Display, fs, path::PathBuf, io::Write};

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

    create_if_not_exists(base.join("microdocker"))
}

pub fn create_if_not_exists(path: PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let exists = fs::exists(&path);
    if let Err(e) = exists {
        return Err(format!("failed to check dir existing: {}", e).into());
    };
    if !exists.unwrap() {
        if let Err(e) = fs::create_dir(&path) {
            return Err(format!("failed to create directory: {}", e).into());
        }
    }

    Ok(path)
}

pub fn init_log(verbose: bool) {
    env_logger::Builder::new()
        .target(env_logger::Target::Stderr)
        .filter_level(if verbose { log::LevelFilter::Trace } else { log::LevelFilter::Info } )
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();
}
