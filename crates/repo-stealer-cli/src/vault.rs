use repo_stealer_vault::{SecretString, Vault};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub const HELP: &str = "Repo Stealer CLI
Usage:
  repo-stealer plan SOURCE.refs DESTINATION.refs
  repo-stealer vault init VAULT
  repo-stealer vault put VAULT NAME INPUT
  repo-stealer vault list VAULT
  repo-stealer vault verify VAULT
  repo-stealer vault export VAULT NAME OUTPUT --plaintext
  repo-stealer vault backup VAULT OUTPUT.rsbackup
  repo-stealer vault verify-backup INPUT.rsbackup
  repo-stealer vault restore INPUT.rsbackup NEW_VAULT

Vault commands prompt for a passphrase. Add --password-stdin for automation
(one passphrase, followed by EOF; never put passwords in argv or environment).
Export writes plaintext to your chosen path. Inputs remain at their source.
Storage and backups create no plaintext temporary files. No Git transfers yet.";

fn password(stdin: bool, confirm: bool) -> Result<SecretString, String> {
    if stdin {
        let mut value = zeroize::Zeroizing::new(String::new());
        io::stdin()
            .take(4097)
            .read_to_string(&mut value)
            .map_err(|_| "cannot read passphrase")?;
        if value.len() > 4096 {
            return Err("passphrase exceeds 4096 bytes".into());
        }
        // SecretString takes ownership; no command-line or environment copy.
        while value.ends_with('\n') || value.ends_with('\r') {
            value.pop();
        }
        if value.is_empty() {
            return Err("passphrase must not be empty".into());
        }
        return Ok(SecretString::from(std::mem::take(&mut *value)));
    }
    let value = rpassword::prompt_password("Vault passphrase: ")
        .map_err(|_| "cannot read terminal passphrase")?;
    if confirm {
        let confirmation = rpassword::prompt_password("Confirm passphrase: ")
            .map_err(|_| "cannot confirm passphrase")?;
        let matches = value == confirmation;
        // Both owned secret allocations are wiped on drop.
        let _confirmation = SecretString::from(confirmation);
        if !matches {
            let _value = SecretString::from(value);
            return Err("passphrases differ".into());
        }
    }
    Ok(SecretString::from(value))
}

pub fn run(args: &[OsString]) -> Result<(), String> {
    let stdin = args.iter().any(|a| a == "--password-stdin");
    let plaintext = args.iter().any(|a| a == "--plaintext");
    let args: Vec<_> = args
        .iter()
        .filter(|a| *a != "--password-stdin" && *a != "--plaintext")
        .collect();
    let command = args.first().and_then(|a| a.to_str()).unwrap_or("");
    let expected = match command {
        "init" | "list" | "verify" | "verify-backup" => 2,
        "backup" | "restore" => 3,
        "put" | "export" => 4,
        _ => return Err(HELP.into()),
    };
    if args.len() != expected || (command == "export") != plaintext {
        return Err(HELP.into());
    }
    let password = password(stdin, command == "init")?;
    let path = Path::new(args[1]);
    let execute = || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match command {
            "init" => {
                Vault::create(path, password)?;
                println!("Encrypted vault created.");
            }
            "verify-backup" => {
                Vault::verify_backup(path, password)?;
                println!("Backup authentication and contents verified.");
            }
            "restore" => {
                Vault::restore(path, Path::new(args[2]), password)?;
                println!("Encrypted vault restored.");
            }
            _ => {
                let mut vault = Vault::open(path, password)?;
                match command {
                    "put" => {
                        let name = args[2].to_str().ok_or("logical name must be UTF-8")?;
                        vault.put(name, File::open(Path::new(args[3]))?)?;
                        println!("Encrypted object stored.");
                    }
                    "list" => {
                        for e in vault.entries() {
                            println!("{}\t{} bytes", e.name, e.size);
                        }
                    }
                    "verify" => {
                        vault.verify()?;
                        println!("Vault authentication and contents verified.");
                    }
                    "backup" => {
                        vault.backup(Path::new(args[2]))?;
                        println!("Encrypted backup created and verified.");
                    }
                    "export" => {
                        let name = args[2].to_str().ok_or("logical name must be UTF-8")?;
                        vault.export_plaintext(name, Path::new(args[3]))?;
                        println!("Plaintext exported to selected output.");
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    };
    execute().map_err(|e| e.to_string())
}
