//! What git asks while it runs, such as a password, put to the user.
//!
//! git and ssh ask through the program named in `GIT_ASKPASS` and `SSH_ASKPASS`, and take the
//! line it prints as the answer. Once [`install`]ed, that program is GitCraft itself: run by git
//! with [`VAR`] set, it hands the question with [`ask`] to the window that ran git, over the
//! loopback interface, where it comes in as a [`Question`] from [`listen`].

use std::ffi::OsString;
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Set when git runs GitCraft to ask something: where to hand the question to.
pub const VAR: &str = "GITCRAFT_ASKPASS";

/// What every `git` runs with, once [`install`]ed.
static ENV: OnceLock<[(&str, OsString); 4]> = OnceLock::new();

/// A question from git, waiting for its answer.
#[derive(Debug)]
pub struct Question {
    /// What git asks, the way it would at a terminal.
    pub prompt: String,
    asker: TcpStream,
}

impl Question {
    /// Whether the answer is a secret, such as a password, rather than a username or a yes or no.
    pub fn secret(&self) -> bool {
        !(self.prompt.starts_with("Username") || self.prompt.contains("(yes/no"))
    }

    /// Gives git `answer`, or has it give up with `None`.
    pub fn answer(mut self, answer: Option<&str>) {
        if let Some(answer) = answer {
            // Failing to write means git gave up already.
            let _ = writeln!(self.asker, "{answer}");
        }
    }
}

/// Starts taking questions: each goes to the receiver, followed by a call to `asked`. Also gives
/// the address to hand them to, for [`install`] and [`ask`].
pub fn listen(asked: impl Fn() + Send + 'static) -> io::Result<(String, Receiver<Question>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    // Only the programs git runs know the token, so that no other program can ask the user
    // anything. Every `RandomState` hashes with keys of its own, which the system makes up.
    let [high, low] = [(); 2].map(|()| RandomState::new().hash_one(0));
    let token = format!("{high:016x}{low:016x}");
    let address = format!("{}/{token}", listener.local_addr()?);
    let (send, questions) = mpsc::channel();
    std::thread::spawn(move || {
        for asker in listener.incoming().flatten() {
            let mut request = String::new();
            let read = asker.set_read_timeout(Some(Duration::from_secs(5)));
            if read.and_then(|()| (&asker).take(1 << 16).read_to_string(&mut request)).is_err() {
                continue;
            }
            let Some(prompt) = request.strip_prefix(&token).and_then(|rest| rest.strip_prefix('\n')) else {
                continue;
            };
            if send.send(Question { prompt: prompt.to_owned(), asker }).is_err() {
                return;
            }
            asked();
        }
    });
    Ok((address, questions))
}

/// Has every `git` from now on ask through `program`, which hands the questions to `address`
/// (see [`listen`]). Only the first call counts: one window answers them all.
pub fn install(program: &Path, address: &str) {
    let program = program.as_os_str();
    let _ = ENV.set([
        ("GIT_ASKPASS", program.to_owned()),
        ("SSH_ASKPASS", program.to_owned()),
        // ssh would ask only without a terminal, and before OpenSSH 8.4 only with a display.
        ("SSH_ASKPASS_REQUIRE", "force".into()),
        (VAR, address.into()),
    ]);
}

/// The environment for asking through the window, once [`install`]ed.
pub(crate) fn env() -> &'static [(&'static str, OsString)] {
    ENV.get().map_or(&[], |env| env)
}

/// Hands `prompt` to the window at `address`, and gives its answer: `None` when there is none.
pub fn ask(address: &str, prompt: &str) -> io::Result<Option<String>> {
    let (at, token) = address.split_once('/').ok_or_else(|| io::Error::other(format!("not an address: {address}")))?;
    let mut window = TcpStream::connect(at)?;
    write!(window, "{token}\n{prompt}")?;
    window.shutdown(Shutdown::Write)?;
    let mut answer = String::new();
    window.read_to_string(&mut answer)?;
    Ok(answer.strip_suffix('\n').map(str::to_owned))
}
