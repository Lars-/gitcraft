//! git and ssh asking the built GitCraft for a password, which hands the question to the window.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use gitcraft_core::askpass::{self, Question};
use gitcraft_core::git::Git;

/// What the window is asked. One test at a time takes them: every `git` asks the same window.
static QUESTIONS: LazyLock<Mutex<Receiver<Question>>> = LazyLock::new(|| {
    let (address, questions) = askpass::listen(|| {}).unwrap();
    askpass::install(Path::new(env!("CARGO_BIN_EXE_gitcraft")), &address);
    Mutex::new(questions)
});

/// Takes the questions, also after a test failed with them.
fn questions() -> MutexGuard<'static, Receiver<Question>> {
    QUESTIONS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Answers the questions git asks, which must be `prompts`, in order.
fn answer(questions: &Receiver<Question>, prompts: &[(&str, Option<&str>)]) {
    for &(prompt, answer) in prompts {
        let question = questions.recv_timeout(Duration::from_secs(30)).expect(prompt);
        assert!(question.prompt.starts_with(prompt), "{:?} is not {prompt:?}", question.prompt);
        question.answer(answer);
    }
}

#[test]
fn git_asks_for_a_username_and_a_password() {
    let questions = questions();
    // A server that wants a password, and tells what it was given.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = server.local_addr().unwrap();
    let (send, given) = mpsc::channel();
    std::thread::spawn(move || {
        for mut client in server.incoming().map(Result::unwrap) {
            let lines = BufReader::new(&client).lines().map(Result::unwrap);
            let request: Vec<_> = lines.take_while(|line| !line.is_empty()).collect();
            let _ = send.send(request.iter().find_map(|line| line.strip_prefix("Authorization: ").map(str::to_owned)));
            let refused = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"test\"\r\n";
            client.write_all(format!("{refused}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
        }
    });
    let url = format!("http://{host}/repo.git");
    let listing = std::thread::spawn(move || Git::new(".").run(&["-c", "credential.helper=", "ls-remote", &url]));

    let (username, password) =
        (format!("Username for 'http://{host}': "), format!("Password for 'http://tess@{host}': "));
    answer(&questions, &[(&username, Some("tess")), (&password, Some("secret"))]);
    let error = listing.join().unwrap().unwrap_err().to_string();
    assert!(error.contains("Authentication failed"), "{error}");
    // `tess:secret` in base64.
    assert!(given.try_iter().any(|given| given.as_deref() == Some("Basic dGVzczpzZWNyZXQ=")));
}

/// Signing a commit with a key that has a passphrase: ssh asks for it the way it does on a push.
/// Windows is left out, because ssh there wants the key's file to be only the user's in a way of
/// its own.
#[cfg(unix)]
#[test]
fn ssh_asks_for_a_passphrase_and_git_gives_up_without_one() {
    let questions = questions();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "secret", "-C", "tess", "-f"])
        .arg(&key)
        .status();
    assert!(made.unwrap().success());
    let git = Git::new(dir.path());
    git.run(&["init", "-q"]).unwrap();
    let key = format!("user.signingKey={}", key.display());
    let config = ["user.name=Tess Ter", "user.email=tess@example.com", "gpg.format=ssh", &key];
    let args: Vec<_> = config
        .into_iter()
        .flat_map(|c| ["-c", c])
        .chain(["commit", "-q", "-S", "--allow-empty", "-m", "Signed"])
        .collect();
    let sign = |git: Git, args: Vec<String>| std::thread::spawn(move || git.run(&args));
    let args: Vec<_> = args.into_iter().map(str::to_owned).collect();

    let signing = sign(git.clone(), args.clone());
    answer(&questions, &[("Enter passphrase", None)]);
    assert!(signing.join().unwrap().is_err(), "signed without the passphrase");
    let signing = sign(git.clone(), args);
    answer(&questions, &[("Enter passphrase", Some("secret"))]);
    signing.join().unwrap().unwrap();
    assert!(git.run(&["cat-file", "commit", "HEAD"]).unwrap().contains("-----BEGIN SSH SIGNATURE-----"));
}
