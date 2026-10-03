//! Finding the `rustc` every object and every link goes through.
//!
//! The objects `ccinrs` makes name the standard library's symbols as one
//! particular `rustc` build mangles them, so they link only with that build's
//! standard library: one `rustc`, the same one every time, is what makes them
//! objects at all. `RUSTC` names it, as it does for Cargo; otherwise it is the
//! `rustc` on `PATH`.

use std::ffi::OsString;
use std::process::Command;

/// The oldest `rustc` that compiles what cinrs generates: 1.99 is where a
/// variadic definition became stable.
const MINIMUM: (u32, u32) = (1, 99);

/// The compiler, as `rustc -vV` describes it.
#[derive(Clone, Debug)]
pub struct Rustc {
    /// What to run.
    pub program: OsString,
    /// `release:` — `1.99.0`, `1.100.0-beta.3`, …
    pub release: String,
    /// `host:`, the triple it compiles for when not told otherwise.
    pub host: String,
    /// `commit-hash:` — what tells two builds of one release apart, which
    /// mangle the standard library's symbols differently.
    pub commit: String,
}

impl Rustc {
    /// Finds `rustc` and checks that it is new enough.
    ///
    /// # Errors
    ///
    /// What is wrong, in a sentence that says what to do about it.
    pub fn locate() -> Result<Self, String> {
        let program = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
        let shown = program.to_string_lossy().into_owned();
        let output = Command::new(&program)
            .arg("-vV")
            .output()
            .map_err(|error| {
                format!(
                    "cannot run '{shown}': {error}. ccinrs compiles through rustc; install Rust \
                     from https://rustup.rs, or name a rustc with RUSTC"
                )
            })?;
        if !output.status.success() {
            return Err(format!("'{shown} -vV' failed"));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name))
                .map(|value| value.trim().to_owned())
        };
        let release = field("release:").ok_or_else(|| format!("'{shown} -vV' names no release"))?;
        let host = field("host:").ok_or_else(|| format!("'{shown} -vV' names no host"))?;
        let commit = field("commit-hash:").unwrap_or_else(|| "unknown".to_owned());
        let version = parse_version(&release)
            .ok_or_else(|| format!("'{shown}' reports a release ccinrs cannot read: {release}"))?;
        if version < MINIMUM {
            return Err(format!(
                "'{shown}' is {release}, and ccinrs needs Rust {}.{} or later",
                MINIMUM.0, MINIMUM.1
            ));
        }
        Ok(Self {
            program,
            release,
            host,
            commit,
        })
    }

    /// A command running this `rustc`.
    pub fn command(&self) -> Command {
        Command::new(&self.program)
    }

    /// Where the standard library for `triple` is, when it is installed,
    /// which is what compiling for it needs: `rustc --print target-libdir`
    /// names the directory whether or not it is there, so it is looked at.
    pub fn target_libdir(&self, triple: &str) -> Option<std::path::PathBuf> {
        let output = self
            .command()
            .args(["--print", "target-libdir", "--target", triple])
            .output()
            .ok()?;
        let dir = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        (output.status.success() && dir.is_dir()).then_some(dir)
    }
}

/// `1.99.0-beta.3` → `(1, 99)`.
fn parse_version(release: &str) -> Option<(u32, u32)> {
    let mut parts = release.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn releases() {
        assert_eq!(parse_version("1.99.0"), Some((1, 99)));
        assert_eq!(parse_version("1.100.0-beta.3"), Some((1, 100)));
        assert_eq!(parse_version("2.0.0-nightly"), Some((2, 0)));
        assert_eq!(parse_version("x"), None);
        assert!(parse_version("1.98.1").unwrap() < (1, 99));
    }
}
