use ncat_core::{DEFAULT_BUDGET, MIB};
use std::{io, time::Duration};

pub(crate) const MAX_CACHE_ENTRIES: usize = 64;
pub(crate) const PREFETCH_STOP_PERCENT: u64 = 80;

#[derive(Clone, Copy, Debug)]
pub struct BackendOptions {
    pub budget: u64,
    /// Pixel/key cap; shared frames also retain their global presentation reservations.
    pub cache_bytes: u64,
    pub prefetch: bool,
    pub workspace_bytes: u64,
    pub task_timeout: Duration,
}
impl Default for BackendOptions {
    fn default() -> Self {
        Self {
            budget: DEFAULT_BUDGET,
            cache_bytes: 24 * MIB,
            prefetch: true,
            workspace_bytes: 128 * MIB,
            task_timeout: Duration::from_secs(15),
        }
    }
}
impl BackendOptions {
    pub fn validate(self) -> io::Result<Self> {
        if self.budget == 0
            || self.budget > 384 * MIB
            || self.cache_bytes > self.budget / 3
            || !(8 * MIB..=384 * MIB).contains(&self.workspace_bytes)
            || !(Duration::from_secs(1)..=Duration::from_secs(120)).contains(&self.task_timeout)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid memory/cache/workspace/timeout settings",
            ));
        }
        Ok(self)
    }
    pub fn from_env() -> io::Result<Self> {
        for key in [
            "NCAT_MEMORY_MIB",
            "NCAT_CACHE_MIB",
            "NCAT_WORKSPACE_MIB",
            "NCAT_TIMEOUT_SECS",
            "NCAT_PREFETCH",
        ] {
            if matches!(std::env::var(key), Err(std::env::VarError::NotUnicode(_))) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, key));
            }
        }
        Self::from_lookup(|key| std::env::var(key).ok())
    }
    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> io::Result<Self> {
        let mut options = Self::default();
        for (key, target) in [
            ("NCAT_MEMORY_MIB", &mut options.budget),
            ("NCAT_CACHE_MIB", &mut options.cache_bytes),
            ("NCAT_WORKSPACE_MIB", &mut options.workspace_bytes),
        ] {
            if let Some(value) = get(key) {
                *target = value
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| n.checked_mul(MIB))
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, key))?;
            }
        }
        if get("NCAT_CACHE_MIB").is_none() {
            options.cache_bytes = options.budget / 8;
        }
        if let Some(value) = get("NCAT_TIMEOUT_SECS") {
            options.task_timeout =
                Duration::from_secs(value.parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "NCAT_TIMEOUT_SECS")
                })?);
        }
        if let Some(value) = get("NCAT_PREFETCH") {
            options.prefetch = match value.as_str() {
                "1" | "true" => true,
                "0" | "false" => false,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "NCAT_PREFETCH")),
            };
        }
        options.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_parse_without_mutating_process_environment() {
        let options = BackendOptions::from_lookup(|key| match key {
            "NCAT_MEMORY_MIB" => Some("64".into()),
            "NCAT_PREFETCH" => Some("0".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(options.cache_bytes, 8 * MIB);
        assert!(!options.prefetch);
        for value in ["-1", "18446744073709551615", "0", "385", "invalid"] {
            assert!(
                BackendOptions::from_lookup(|key| (key == "NCAT_MEMORY_MIB").then(|| value.into()))
                    .is_err()
            );
        }
        assert!(
            BackendOptions {
                cache_bytes: DEFAULT_BUDGET,
                ..BackendOptions::default()
            }
            .validate()
            .is_err()
        );
    }
}
