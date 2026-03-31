use std::env;

pub struct AppConfig {
    pub workers: usize,
    pub max_concurrent_ocr: usize,
    pub ocr_timeout_secs: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            workers: env::var("LA_TAUPE_WORKERS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            max_concurrent_ocr: env::var("LA_TAUPE_MAX_CONCURRENT_OCR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            ocr_timeout_secs: env::var("LA_TAUPE_OCR_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_default_config() {
        env::remove_var("LA_TAUPE_WORKERS");
        env::remove_var("LA_TAUPE_MAX_CONCURRENT_OCR");
        env::remove_var("LA_TAUPE_OCR_TIMEOUT_SECS");

        let config = AppConfig::from_env();
        assert_eq!(config.workers, 3);
        assert_eq!(config.max_concurrent_ocr, 3);
        assert_eq!(config.ocr_timeout_secs, 120);
    }

    #[test]
    #[serial]
    fn test_custom_config() {
        env::set_var("LA_TAUPE_WORKERS", "5");
        env::set_var("LA_TAUPE_MAX_CONCURRENT_OCR", "2");
        env::set_var("LA_TAUPE_OCR_TIMEOUT_SECS", "60");

        let config = AppConfig::from_env();
        assert_eq!(config.workers, 5);
        assert_eq!(config.max_concurrent_ocr, 2);
        assert_eq!(config.ocr_timeout_secs, 60);

        env::remove_var("LA_TAUPE_WORKERS");
        env::remove_var("LA_TAUPE_MAX_CONCURRENT_OCR");
        env::remove_var("LA_TAUPE_OCR_TIMEOUT_SECS");
    }

    #[test]
    #[serial]
    fn test_invalid_values_use_defaults() {
        env::set_var("LA_TAUPE_WORKERS", "not_a_number");
        env::set_var("LA_TAUPE_MAX_CONCURRENT_OCR", "");
        env::set_var("LA_TAUPE_OCR_TIMEOUT_SECS", "-1");

        let config = AppConfig::from_env();
        assert_eq!(config.workers, 3);
        assert_eq!(config.max_concurrent_ocr, 3);
        assert_eq!(config.ocr_timeout_secs, 120);

        env::remove_var("LA_TAUPE_WORKERS");
        env::remove_var("LA_TAUPE_MAX_CONCURRENT_OCR");
        env::remove_var("LA_TAUPE_OCR_TIMEOUT_SECS");
    }
}
