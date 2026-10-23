use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};

use crate::consts::APP_VERSION;

pub(crate) fn initialize(config_path: &Path) -> Result<()> {
    let credentials_path = config_path.with_extension("credentials");
    if config_path == credentials_path {
        return Err(anyhow!(
            "config path and generated credentials path must be different"
        ));
    }
    if config_path.exists() {
        return Err(anyhow!(
            "config file already exists: {} (refusing to overwrite)",
            config_path.display()
        ));
    }
    if credentials_path.exists() {
        return Err(anyhow!(
            "credentials file already exists: {} (refusing to overwrite)",
            credentials_path.display()
        ));
    }

    println!("Create a No-IP Client configuration");
    let username = prompt("No-IP username or email: ")?;
    if username.trim().is_empty() {
        return Err(anyhow!("No-IP username or email cannot be empty"));
    }
    let hostname = prompt("Hostname to update: ")?;
    if hostname.trim().is_empty() {
        return Err(anyhow!("hostname cannot be empty"));
    }
    let ip_version = prompt_ip_version()?;
    let password = rpassword::prompt_password("No-IP password (input hidden): ")
        .context("reading No-IP password")?;
    if password.trim().is_empty() {
        return Err(anyhow!("No-IP password cannot be empty"));
    }
    if password.trim() != password || password.contains('\n') || password.contains('\r') {
        return Err(anyhow!(
            "password cannot start or end with whitespace or contain a line break"
        ));
    }

    let config = render_config(
        hostname.trim(),
        username.trim(),
        ip_version,
        &absolute_path(&credentials_path)?,
    );
    let credentials = format!("{}\n{password}\n", username.trim());

    let parent = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating config directory {}", parent.display()))?;

    write_new_file(&credentials_path, credentials.as_bytes())?;
    if let Err(error) = write_new_file(config_path, config.as_bytes()) {
        if let Err(cleanup_error) = fs::remove_file(&credentials_path) {
            return Err(error).context(format!(
                "also failed to remove credentials file {}: {cleanup_error}",
                credentials_path.display()
            ));
        }
        return Err(error);
    }

    println!("Created config: {}", config_path.display());
    println!("Created credentials: {}", credentials_path.display());
    println!("Both files are created with owner-only permissions on Unix.");
    println!(
        "Run `noipclient --config {}` to start.",
        config_path.display()
    );
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("writing prompt")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("reading setup input")?;
    Ok(input.trim().to_owned())
}

fn prompt_ip_version() -> Result<u8> {
    loop {
        let input = prompt("Address family [4/6] (default 4): ")?;
        match input.as_str() {
            "" | "4" => return Ok(4),
            "6" => return Ok(6),
            _ => println!("Enter 4 or 6."),
        }
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn render_config(
    hostname: &str,
    username: &str,
    ip_version: u8,
    credentials_path: &Path,
) -> String {
    format!(
        "[noip]\n\
         user_agent = {}\n\
         credentials_file = {}\n\
         \n\
         [[hosts]]\n\
         hostname = {}\n\
         ip_version = {ip_version}\n",
        toml::Value::String(format!("noipclient/{APP_VERSION} {username}")),
        toml::Value::String(credentials_path.to_string_lossy().into()),
        toml::Value::String(hostname.to_owned()),
    )
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {} (refusing to overwrite)", path.display()))?;
    if let Err(error) = file.write_all(contents) {
        drop(file);
        return match fs::remove_file(path) {
            Ok(()) => Err(error).with_context(|| format!("writing {}", path.display())),
            Err(cleanup_error) => Err(error).context(format!(
                "writing {} failed and partial file cleanup failed: {cleanup_error}",
                path.display()
            )),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_file_is_derived_from_config_name() {
        assert_eq!(
            Path::new("config/noip.toml").with_extension("credentials"),
            Path::new("config/noip.credentials")
        );
    }

    #[test]
    fn generated_config_is_valid_toml_and_quotes_user_values() {
        let generated = render_config(
            "my\"host.ddns.net",
            "user\"name@example.com",
            6,
            Path::new("/tmp/noip.credentials"),
        );
        let parsed: toml::Value = toml::from_str(&generated).unwrap();
        assert_eq!(
            parsed["hosts"][0]["hostname"].as_str(),
            Some("my\"host.ddns.net")
        );
        assert_eq!(parsed["hosts"][0]["ip_version"].as_integer(), Some(6));
        assert_eq!(
            parsed["noip"]["credentials_file"].as_str(),
            Some("/tmp/noip.credentials")
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_files_are_owner_only() {
        use std::{os::unix::fs::PermissionsExt, time::SystemTime};

        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("noipclient-init-{unique}"));
        write_new_file(&path, b"secret").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
    }
}
