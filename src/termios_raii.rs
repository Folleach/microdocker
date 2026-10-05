use std::{io, os::fd::AsRawFd};

use crate::{common::WithErrExt, libc_wrappers::{cfmakeraw, tcgetattr, tcsetattr}};


pub struct TermiosState {
    original: libc::termios
}

impl TermiosState {
    pub fn make_raw() -> Result<TermiosState, String> {
        let mut termios = tcgetattr(&io::stdin()).with_err("failed to take termios")?;
        let orig = termios.clone();

        cfmakeraw(&mut termios);
        tcsetattr(&io::stdin().as_raw_fd(), &mut termios).with_err("failed to set raw mode to the controller terminal")?;

        Ok(TermiosState { 
            original: orig
        })
    }
}

impl Drop for TermiosState {
    fn drop(&mut self) {
        _ = tcsetattr(&io::stdin().as_raw_fd(), &self.original);
    }
}

