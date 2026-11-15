//! The test SFTP server as a program, for native checks of the desktop (ADR 0037). Dev only.
//!
//! `cargo run -p dm-test-fixtures --example sftp_server -- <port-file> <user> <password> <file>`
//! serves `<file>` as `/srv/<file name>` with a fixed Ed25519 host key, and writes the port and the
//! key's fingerprint to `<port-file>`.

#![allow(clippy::print_stdout, clippy::unwrap_used)]

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (out, user, password, file) = (&args[1], &args[2], &args[3], &args[4]);
    let srv = dm_test_fixtures::sftp::SftpServer::start(1, user, password).await;
    let body = std::fs::read(file).unwrap();
    let name = std::path::Path::new(file)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    srv.set(&format!("srv/{name}"), body, 1_700_000_000);
    let url = srv.url(&format!("srv/{name}"));
    std::fs::write(out, format!("{url}\n{}\n", srv.fingerprint())).unwrap();
    println!("serving {url}");
    tokio::signal::ctrl_c().await.unwrap();
}
