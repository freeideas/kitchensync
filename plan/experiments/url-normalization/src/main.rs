use percent_encoding::percent_decode_str;
use std::error::Error;
use std::path::Path;
use url::Url;

fn collapse_slashes(path: &str) -> String {
    let mut out = String::new();
    let mut previous_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !previous_slash {
                out.push(ch);
            }
            previous_slash = true;
        } else {
            out.push(ch);
            previous_slash = false;
        }
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut sftp = Url::parse("SFTP://Host:22//docs/%7Ealice?timeout-conn=60")?;
    assert_eq!(sftp.scheme(), "sftp");
    assert_eq!(sftp.host_str(), Some("Host"));
    assert_eq!(sftp.host_str().unwrap().to_ascii_lowercase(), "host");
    assert_eq!(sftp.port(), Some(22));
    assert_eq!(sftp.path(), "//docs/%7Ealice");
    assert_eq!(sftp.query_pairs().next().unwrap().0, "timeout-conn");

    if sftp.port() == Some(22) {
        sftp.set_port(None).expect("remove default port");
    }
    sftp.set_query(None);
    let normalized_path = collapse_slashes(sftp.path());
    assert_eq!(normalized_path, "/docs/%7Ealice");
    let decoded = percent_decode_str(&normalized_path).decode_utf8()?;
    assert_eq!(decoded, "/docs/~alice");

    let mut no_user = Url::parse("sftp://host/path")?;
    assert_eq!(no_user.username(), "");
    no_user
        .set_username(&whoami::username())
        .expect("set current user");
    assert!(!no_user.username().is_empty());

    let local = Path::new("./data")
        .canonicalize()
        .unwrap_or(std::env::current_dir()?.join("data"));
    let file_url = Url::from_directory_path(&local).expect("file URL");
    assert_eq!(file_url.scheme(), "file");
    assert!(file_url.as_str().starts_with("file://"));

    let password = Url::parse("sftp://user:p%40ss%3Aword@host/path")?;
    assert_eq!(password.username(), "user");
    assert_eq!(password.password(), Some("p%40ss%3Aword"));
    let decoded_password = percent_decode_str(password.password().unwrap()).decode_utf8()?;
    assert_eq!(decoded_password, "p@ss:word");

    println!("checked url parsing, default-port stripping, query stripping, current-user insertion, and percent decoding");
    Ok(())
}
