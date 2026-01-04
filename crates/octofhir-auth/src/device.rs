use axum::http::HeaderMap;

/// Parsed device information from User-Agent header
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub browser: String,
    pub os: String,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Desktop,
    Mobile,
    Tablet,
    Unknown,
}

impl DeviceInfo {
    /// Generate a human-readable device name like "Chrome on macOS"
    pub fn to_display_name(&self) -> String {
        format!("{} on {}", self.browser, self.os)
    }
}

/// Extract User-Agent header value from HTTP headers
pub fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

/// Parse device information from User-Agent string
pub fn parse_device_info(user_agent: &str) -> DeviceInfo {
    let browser = detect_browser(user_agent);
    let os = detect_os(user_agent);
    let device_type = detect_device_type(user_agent);

    DeviceInfo {
        browser,
        os,
        device_type,
    }
}

/// Generate human-readable device name from User-Agent
pub fn generate_device_name(user_agent: Option<&str>) -> String {
    match user_agent {
        Some(ua) => parse_device_info(ua).to_display_name(),
        None => "Unknown Device".to_string(),
    }
}

/// Detect browser from User-Agent string
fn detect_browser(ua: &str) -> String {
    let ua_lower = ua.to_lowercase();

    // Check for specific browsers (order matters - Chrome contains Safari, Edge contains Chrome)
    if ua_lower.contains("edg/") || ua_lower.contains("edge/") {
        "Edge".to_string()
    } else if ua_lower.contains("opr/") || ua_lower.contains("opera") {
        "Opera".to_string()
    } else if ua_lower.contains("chrome/") {
        "Chrome".to_string()
    } else if ua_lower.contains("safari/") && !ua_lower.contains("chrome") {
        "Safari".to_string()
    } else if ua_lower.contains("firefox/") {
        "Firefox".to_string()
    } else if ua_lower.contains("msie") || ua_lower.contains("trident/") {
        "Internet Explorer".to_string()
    } else {
        "Unknown Browser".to_string()
    }
}

/// Detect operating system from User-Agent string
fn detect_os(ua: &str) -> String {
    let ua_lower = ua.to_lowercase();

    if ua_lower.contains("windows nt 10") {
        "Windows 10".to_string()
    } else if ua_lower.contains("windows nt 11") {
        "Windows 11".to_string()
    } else if ua_lower.contains("windows") {
        "Windows".to_string()
    } else if ua_lower.contains("mac os x") || ua_lower.contains("macintosh") {
        "macOS".to_string()
    } else if ua_lower.contains("iphone") {
        "iOS".to_string()
    } else if ua_lower.contains("ipad") {
        "iPadOS".to_string()
    } else if ua_lower.contains("android") {
        "Android".to_string()
    } else if ua_lower.contains("linux") {
        "Linux".to_string()
    } else if ua_lower.contains("cros") {
        "Chrome OS".to_string()
    } else {
        "Unknown OS".to_string()
    }
}

/// Detect device type from User-Agent string
fn detect_device_type(ua: &str) -> DeviceType {
    let ua_lower = ua.to_lowercase();

    if ua_lower.contains("mobile") || ua_lower.contains("iphone") || ua_lower.contains("android") {
        // Check if it's actually a tablet
        if ua_lower.contains("tablet") || ua_lower.contains("ipad") {
            DeviceType::Tablet
        } else {
            DeviceType::Mobile
        }
    } else if ua_lower.contains("ipad") || ua_lower.contains("tablet") {
        DeviceType::Tablet
    } else if ua_lower.contains("windows")
        || ua_lower.contains("macintosh")
        || ua_lower.contains("linux")
        || ua_lower.contains("cros")
    {
        DeviceType::Desktop
    } else {
        DeviceType::Unknown
    }
}

/// Extract IP address from headers (supports proxies)
pub fn extract_ip_address(headers: &HeaderMap) -> Option<String> {
    // Try X-Forwarded-For first (if behind proxy/load balancer)
    // X-Forwarded-For can contain multiple IPs: "client, proxy1, proxy2"
    // Take the first one (original client IP)
    if let Some(forwarded) = headers.get("x-forwarded-for")
        && let Ok(value) = forwarded.to_str()
        && let Some(client_ip) = value.split(',').next()
    {
        return Some(client_ip.trim().to_string());
    }

    // Try X-Real-IP (common with nginx)
    if let Some(real_ip) = headers.get("x-real-ip")
        && let Ok(value) = real_ip.to_str()
    {
        return Some(value.to_string());
    }

    // If no proxy headers, we'd need ConnectInfo from Axum
    // This should be handled at the handler level
    None
}

/// Generate a device fingerprint (hash of user agent + IP for deduplication)
pub fn generate_device_fingerprint(user_agent: Option<&str>, ip: Option<&str>) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(user_agent.unwrap_or(""));
    hasher.update(ip.unwrap_or(""));

    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_browser_chrome() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        assert_eq!(detect_browser(ua), "Chrome");
    }

    #[test]
    fn test_detect_browser_firefox() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/121.0";
        assert_eq!(detect_browser(ua), "Firefox");
    }

    #[test]
    fn test_detect_browser_safari() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Safari/605.1.15";
        assert_eq!(detect_browser(ua), "Safari");
    }

    #[test]
    fn test_detect_browser_edge() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0";
        assert_eq!(detect_browser(ua), "Edge");
    }

    #[test]
    fn test_detect_os_macos() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";
        assert_eq!(detect_os(ua), "macOS");
    }

    #[test]
    fn test_detect_os_windows() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";
        assert_eq!(detect_os(ua), "Windows 10");
    }

    #[test]
    fn test_detect_os_ios() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X)";
        assert_eq!(detect_os(ua), "iOS");
    }

    #[test]
    fn test_detect_os_android() {
        let ua = "Mozilla/5.0 (Linux; Android 13; Pixel 7)";
        assert_eq!(detect_os(ua), "Android");
    }

    #[test]
    fn test_detect_device_type_desktop() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)";
        assert_eq!(detect_device_type(ua), DeviceType::Desktop);
    }

    #[test]
    fn test_detect_device_type_mobile() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X)";
        assert_eq!(detect_device_type(ua), DeviceType::Mobile);
    }

    #[test]
    fn test_detect_device_type_tablet() {
        let ua = "Mozilla/5.0 (iPad; CPU OS 17_2 like Mac OS X)";
        assert_eq!(detect_device_type(ua), DeviceType::Tablet);
    }

    #[test]
    fn test_generate_device_name() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let name = generate_device_name(Some(ua));
        assert_eq!(name, "Chrome on macOS");
    }

    #[test]
    fn test_generate_device_name_none() {
        let name = generate_device_name(None);
        assert_eq!(name, "Unknown Device");
    }

    #[test]
    fn test_parse_device_info() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1";
        let info = parse_device_info(ua);
        assert_eq!(info.browser, "Safari");
        assert_eq!(info.os, "iOS");
        assert_eq!(info.device_type, DeviceType::Mobile);
    }

    #[test]
    fn test_device_fingerprint_consistency() {
        let fp1 = generate_device_fingerprint(Some("test-ua"), Some("192.168.1.1"));
        let fp2 = generate_device_fingerprint(Some("test-ua"), Some("192.168.1.1"));
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_device_fingerprint_different() {
        let fp1 = generate_device_fingerprint(Some("test-ua-1"), Some("192.168.1.1"));
        let fp2 = generate_device_fingerprint(Some("test-ua-2"), Some("192.168.1.1"));
        assert_ne!(fp1, fp2);
    }
}
