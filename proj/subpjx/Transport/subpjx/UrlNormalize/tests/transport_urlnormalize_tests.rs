use transport_urlnormalize::UrlNormalize;

fn subject() -> std::sync::Arc<dyn UrlNormalize> {
    transport_urlnormalize::new()
}

fn os_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

// 003.1 -- Normalizing a URL lowercases the scheme.
#[test]
fn req_003_1_lowercase_scheme() {
    let s = subject();
    let result = s.normalize("SFTP://host/path");
    assert!(result.starts_with("sftp://"), "scheme must be lowercase: {result}");
}

// 003.2 -- Normalizing a URL lowercases the hostname.
#[test]
fn req_003_2_lowercase_hostname() {
    let s = subject();
    let result = s.normalize("sftp://MyHost/path");
    assert!(
        result.contains("@myhost/") || result.contains("//myhost/"),
        "hostname must be lowercase: {result}"
    );
}

// 003.3 -- Normalizing an SFTP URL that names port 22 removes the port.
#[test]
fn req_003_3_removes_default_sftp_port() {
    let s = subject();
    let result = s.normalize("sftp://host:22/path");
    assert!(!result.contains(":22"), "port 22 must be removed: {result}");
}

// 003.4 -- Normalizing a URL collapses consecutive slashes in the path to a single slash.
#[test]
fn req_003_4_collapses_consecutive_slashes() {
    let s = subject();
    let result = s.normalize("sftp://host//a//b");
    // Check only the path portion (after scheme + authority) to avoid false matches on "://user".
    let path = result.find("://")
        .and_then(|i| result[i + 3..].find('/').map(|j| &result[i + 3 + j..]))
        .unwrap_or(&result);
    assert!(!path.contains("//"), "consecutive slashes must collapse: {result}");
}

// 003.5 -- Normalizing a URL removes a trailing slash from the path.
#[test]
fn req_003_5_removes_trailing_slash() {
    let s = subject();
    let result = s.normalize("sftp://host/path/");
    assert!(!result.ends_with('/'), "trailing slash must be removed: {result}");
}

// 003.6 -- Normalizing a bare path with no scheme converts it to a file:// URL.
#[test]
fn req_003_6_bare_path_to_file_url() {
    let s = subject();
    let result = s.normalize("/absolute/path");
    assert!(result.starts_with("file://"), "bare path must become file:// URL: {result}");
}

// 003.7 -- Normalizing a file:// URL resolves its path to an absolute path from the current working directory.
#[test]
fn req_003_7_file_url_absolute_path_preserved() {
    let s = subject();
    let result = s.normalize("file:///home/user/data");
    assert_eq!(result, "file:///home/user/data");
}

// 003.8 -- Normalizing a URL percent-decodes unreserved characters.
#[test]
fn req_003_8_percent_decodes_unreserved_characters() {
    let s = subject();
    // %7E is '~', an unreserved character; it must appear decoded in the output.
    let result = s.normalize("sftp://host/%7Ename");
    assert!(result.contains("/~name"), "unreserved %7E must decode to '~': {result}");
}

// 003.9 -- Normalizing a URL strips query-string parameters.
#[test]
fn req_003_9_strips_query_string() {
    let s = subject();
    let result = s.normalize("sftp://host/path?key=value&other=123");
    assert!(!result.contains('?'), "query string must be stripped: {result}");
    assert!(!result.contains("key="), "query parameters must be removed: {result}");
}

// 003.10 -- Normalizing an SFTP URL with no username inserts the current OS user as the username.
#[test]
fn req_003_10_sftp_inserts_os_user_when_absent() {
    let s = subject();
    let user = os_user();
    let result = s.normalize("sftp://somehost/path");
    assert!(
        result.contains(&format!("{}@somehost", user)),
        "OS user must be inserted before the host: {result}"
    );
}

// 003.10 (complement) -- An SFTP URL that already names a username keeps it unchanged.
#[test]
fn req_003_10_sftp_preserves_existing_username() {
    let s = subject();
    let result = s.normalize("sftp://bob@somehost/path");
    assert!(result.contains("bob@somehost"), "existing username must be preserved: {result}");
}

// 003.11 -- Normalizing c:/photos/ produces file:///c:/photos.
#[test]
fn req_003_11_windows_drive_path() {
    let s = subject();
    assert_eq!(s.normalize("c:/photos/"), "file:///c:/photos");
}

// 003.12 -- Normalizing ./data from CWD produces file:///<cwd>/data.
#[test]
fn req_003_12_relative_bare_path_resolves_against_cwd() {
    let s = subject();
    let cwd = std::env::current_dir().expect("current_dir must be available");
    let expected = format!("file://{}", cwd.join("data").display());
    let result = s.normalize("./data");
    assert_eq!(result, expected);
}

// 003.13 -- Normalizing SFTP://Host:22/path/ produces sftp://host/path (lowercase scheme and
// host, port 22 removed, trailing slash removed).
#[test]
fn req_003_13_combined_scheme_host_port_slash_normalization() {
    let s = subject();
    let result = s.normalize("SFTP://Host:22/path/");
    assert!(result.starts_with("sftp://"), "scheme must be lowercase: {result}");
    assert!(result.contains("host/path"), "host must be lowercase and trailing slash removed: {result}");
    assert!(!result.contains(":22"), "default SFTP port must be removed: {result}");
    assert!(!result.ends_with('/'), "trailing slash must be removed: {result}");
}

// 003.14 -- Normalizing sftp://host//docs/ produces sftp://host/docs.
#[test]
fn req_003_14_consecutive_slashes_and_trailing_slash() {
    let s = subject();
    let result = s.normalize("sftp://host//docs/");
    assert!(result.ends_with("/docs"), "path must end with /docs: {result}");
    assert!(!result.contains("//docs"), "consecutive slashes must collapse: {result}");
}

// 003.15 -- Normalizing sftp://host/path?timeout-conn=60 produces sftp://host/path.
#[test]
fn req_003_15_sftp_query_string_stripped() {
    let s = subject();
    let result = s.normalize("sftp://host/path?timeout-conn=60");
    assert!(result.ends_with("/path"), "path must be preserved without query: {result}");
    assert!(!result.contains('?'), "query string must be stripped: {result}");
}

// 003.16 -- Normalizing sftp://host/path as the current OS user produces sftp://<user>@host/path.
#[test]
fn req_003_16_sftp_uses_current_os_user() {
    let s = subject();
    let user = os_user();
    let expected = format!("sftp://{}@host/path", user);
    let result = s.normalize("sftp://host/path");
    assert_eq!(result, expected);
}
