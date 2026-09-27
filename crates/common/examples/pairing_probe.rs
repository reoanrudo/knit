//! Isolated enrollment test. Uses an ephemeral token and never reads/writes the
//! real credential store. Run server on Mac and client <ip> <code> on Windows.
use tsunagu_common::{credentials, pairing};
fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("server") => {
            let invitation = pairing::Invitation::open(credentials::generate()?, 24900)?;
            println!("TEST CODE: {}", invitation.code);
            loop {
                match invitation
                    .events
                    .recv_timeout(std::time::Duration::from_secs(305))
                {
                    Ok(pairing::Event::Registered(_)) => {
                        println!("PASS: authenticated peer acknowledged test credential");
                        break;
                    }
                    Ok(pairing::Event::AttemptFailed(_)) => {
                        println!("REJECTED: authentication attempt failed")
                    }
                    event => {
                        return Err(std::io::Error::other(format!(
                            "test not completed: {event:?}"
                        )))
                    }
                }
            }
        }
        Some("client") if args.len() == 4 => {
            let address = pairing::manual_address(&args[2])?;
            let token = pairing::enroll(address, &args[3], |token, _| {
                credentials::parse_key(token)?;
                // Deliberately memory-only: do not alter the user's saved key.
                Ok(())
            })?;
            assert_eq!(token.len(), 64);
            println!(
                "PASS: received authenticated test credential; no persistent settings changed"
            );
        }
        _ => {
            return Err(std::io::Error::other(
                "usage: pairing_probe server | client <ip> <test-code>",
            ))
        }
    }
    Ok(())
}
