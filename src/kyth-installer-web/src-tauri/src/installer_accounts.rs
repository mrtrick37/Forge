//! Bounded offline installer-account creation.
//!
//! The password hash is accepted only in the operation body (stdin) and is
//! never placed in argv, logs, or a diagnostic response.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_USERNAME: usize = 32;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct CreateUserInput {
    pub deploy_root: String,
    pub target_root: String,
    pub username: String,
    pub password_hash: String,
}

/// Hash a frontend password without placing it in argv, logs, or a durable
/// request object. The native daemon consumes plaintext only long enough to
/// feed the fixed SHA-512 crypt operation through stdin.
pub(crate) fn hash_password(password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err(
            "Password cannot be empty. Return to the Configure step and re-enter it.".into(),
        );
    }
    if password.contains('\0') {
        return Err("Password contains an unsupported character".into());
    }
    let mut child = Command::new("/usr/bin/openssl")
        .args(["passwd", "-6", "-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not hash password: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(password.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("could not provide password to hasher: {error}"));
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not wait for password hasher: {error}"))?;
    if !output.status.success() {
        return Err("password hashing failed".into());
    }
    let hash = String::from_utf8(output.stdout)
        .map_err(|_| "password hashing returned non-UTF-8 output".to_string())?;
    let hash = hash.trim();
    if !hash.starts_with("$6$") || hash.contains(['\n', '\r', '\0']) {
        return Err("password hashing returned an invalid SHA-512 crypt value".into());
    }
    Ok(hash.to_string())
}

fn absolute_tree(value: &str, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{label} must be an absolute path without parent traversal"
        ));
    }
    Ok(path)
}

pub fn validate(input: &CreateUserInput) -> Result<(PathBuf, PathBuf), String> {
    let deploy = absolute_tree(&input.deploy_root, "deploy_root")?;
    let target = absolute_tree(&input.target_root, "target_root")?;
    let username = input.username.trim();
    if username.is_empty()
        || username.len() > MAX_USERNAME
        || username.starts_with('-')
        || !username
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphanumeric() || (matches!(b, b'_' | b'-') && i > 0))
    {
        return Err("username contains unsupported characters".into());
    }
    if input.password_hash.is_empty() || input.password_hash.contains(['\n', '\r', '\0']) {
        return Err("password_hash must be a single non-empty line".into());
    }
    Ok((deploy, target))
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed"))
    }
}

fn replace_shadow_hash(path: &Path, username: &str, hash: &str) -> Result<(), String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("could not read installed shadow: {e}"))?;
    let mut found = false;
    let mut output = String::new();
    for line in content.lines() {
        if let Some((name, rest)) = line.split_once(':') {
            if name == username {
                let mut fields: Vec<&str> = rest.split(':').collect();
                if fields.is_empty() {
                    return Err("installed shadow record is malformed".into());
                }
                fields[0] = hash;
                output.push_str(name);
                output.push(':');
                output.push_str(&fields.join(":"));
                found = true;
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    if !found {
        return Err(format!(
            "user {username:?} not found in shadow after useradd"
        ));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .mode(0o000)
        .open(path)
        .map_err(|e| format!("could not open installed shadow: {e}"))?;
    file.write_all(output.as_bytes())
        .map_err(|e| format!("could not write installed shadow: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("could not sync installed shadow: {e}"))?;
    Ok(())
}

pub fn apply(input: CreateUserInput) -> Result<(), String> {
    let (deploy, target) = validate(&input)?;
    let etc = deploy.join("etc");
    let shadow = etc.join("shadow");
    let home = target
        .join("ostree/deploy/default/var/home")
        .join(&input.username);
    run(
        "/usr/sbin/useradd",
        &[
            "--root",
            deploy.to_str().ok_or("deploy_root is not valid UTF-8")?,
            "-M",
            "-G",
            "wheel,video,audio,render",
            "-s",
            "/bin/bash",
            &input.username,
        ],
    )?;
    replace_shadow_hash(&shadow, &input.username, &input.password_hash)?;
    fs::create_dir_all(&home).map_err(|e| format!("could not create user home: {e}"))?;
    let passwd = fs::read_to_string(etc.join("passwd"))
        .map_err(|e| format!("could not read installed passwd: {e}"))?;
    let (uid, gid) = passwd
        .lines()
        .find_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            (fields.first() == Some(&input.username.as_str()) && fields.len() > 3)
                .then(|| (fields[2].to_owned(), fields[3].to_owned()))
        })
        .ok_or_else(|| "user not found in passwd after useradd".to_string())?;
    let ownership = format!("{uid}:{gid}");
    run(
        "/usr/bin/chown",
        &[&ownership, home.to_str().ok_or("home is not UTF-8")?],
    )?;
    run(
        "/usr/bin/chmod",
        &["700", home.to_str().ok_or("home is not UTF-8")?],
    )?;
    let skel = etc.join("skel");
    if skel.is_dir() {
        run(
            "/usr/bin/cp",
            &[
                "-rT",
                skel.to_str().ok_or("skel is not UTF-8")?,
                home.to_str().ok_or("home is not UTF-8")?,
            ],
        )?;
        run(
            "/usr/bin/chown",
            &["-R", &ownership, home.to_str().ok_or("home is not UTF-8")?],
        )?;
    }
    let _ = Command::new("/usr/bin/restorecon")
        .args(["-RF", home.to_str().ok_or("home is not UTF-8")?])
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CreateUserInput {
        CreateUserInput {
            deploy_root: "/target/deploy".into(),
            target_root: "/target".into(),
            username: "kyth_user".into(),
            password_hash: "$6$hash".into(),
        }
    }

    #[test]
    fn validates_bounded_account_request() {
        assert!(validate(&input()).is_ok());
    }

    #[test]
    fn hashes_password_through_stdin_only() {
        let hash = hash_password("native-password").expect("openssl should hash a password");
        assert!(hash.starts_with("$6$"));
        assert!(!hash.contains("native-password"));
    }

    #[test]
    fn rejects_paths_and_usernames_that_escape_contract() {
        let mut value = input();
        value.deploy_root = "relative".into();
        assert!(validate(&value).is_err());
        let mut value = input();
        value.username = "bad;id".into();
        assert!(validate(&value).is_err());
        let mut value = input();
        value.password_hash = "hash\nleak".into();
        assert!(validate(&value).is_err());
    }

    #[test]
    fn replaces_only_selected_shadow_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shadow");
        fs::write(&path, "root:!:x\nkyth_user:!:x\n").unwrap();
        replace_shadow_hash(&path, "kyth_user", "$6$new").unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "root:!:x\nkyth_user:$6$new:x\n"
        );
    }
}
