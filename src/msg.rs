use std::io;
use std::io::BufRead;
use std::io::Read;
use std::process;

use serde;
use serde_json;

use error::*;

/// Messages returned from a cargo sub-command.
pub struct MessageIter(InnerMessageIter);

struct InnerMessageIter {
    done: bool,
    child: process::Child,
    stdout: io::BufReader<process::ChildStdout>,
    stderr: io::BufReader<process::ChildStderr>,
}

impl MessageIter {
    pub(crate) fn from_command(mut cmd: process::Command) -> CargoResult<Self> {
        let mut child = cmd
            .stdout(process::Stdio::piped())
            .stderr(process::Stdio::piped())
            .spawn()
            .map_err(|e| CargoError::new(ErrorKind::InvalidCommand).set_cause(e))?;
        let stdout = child.stdout.take().expect("piped above");
        let stdout = io::BufReader::new(stdout);
        let stderr = child.stderr.take().expect("piped above");
        let stderr = io::BufReader::new(stderr);
        let msgs = InnerMessageIter {
            done: false,
            child,
            stdout,
            stderr,
        };
        Ok(MessageIter(msgs))
    }

    #[inline]
    fn next_msg(&mut self) -> CargoResult<Option<Message>> {
        let mut content = String::new();
        let len = self
            .0
            .stdout
            .read_line(&mut content)
            .map_err(|e| CargoError::new(ErrorKind::InvalidOutput).set_cause(e))?;
        if 0 < len {
            Ok(Some(Message(content)))
        } else {
            let status = self
                .0
                .child
                .wait()
                .map_err(|e| CargoError::new(ErrorKind::InvalidOutput).set_cause(e))?;
            if !status.success() && !self.0.done {
                self.0.done = true;

                let mut data = vec![];
                self.0
                    .stderr
                    .read_to_end(&mut data)
                    .map_err(|e| CargoError::new(ErrorKind::InvalidOutput).set_cause(e))?;
                let err = CargoError::new(ErrorKind::CommandFailed)
                    .set_context(String::from_utf8_lossy(&data));
                Err(err)
            } else {
                self.0.done = true;
                Ok(None)
            }
        }
    }
}

impl Drop for MessageIter {
    fn drop(&mut self) {
        if !self.0.done {
            let _ = self.0.child.wait();
        }
    }
}

impl Iterator for MessageIter {
    type Item = CargoResult<Message>;

    #[inline]
    fn next(&mut self) -> Option<CargoResult<Message>> {
        match self.next_msg() {
            Ok(Some(x)) => Some(Ok(x)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// An individual message from a cargo sub-command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message(String);

impl Message {
    /// Deserialize the message.
    pub fn convert<'a, T>(&'a self) -> CargoResult<T>
    where
        T: serde::Deserialize<'a>,
    {
        let data = serde_json::from_str(self.0.as_str())
            .map_err(|e| CargoError::new(ErrorKind::InvalidOutput).set_cause(e))?;
        Ok(data)
    }
}
