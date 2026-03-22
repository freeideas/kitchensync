pub const HELP_TEXT: &str = "\
Usage: kitchensync [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path or c:\\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon \u{2014} this peer's state wins all conflicts
  -<peer>                          Subordinate \u{2014} overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  \"sftp://host/path?mc=5\"          Max connections for this URL
  \"sftp://host/path?ct=60\"         Connection timeout for this URL
  \"sftp://host/path?mc=5&ct=60\"    Both

Options:
  -h, --help, /?                      Show this help
  --mc N             Max concurrent connections per URL (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  -vl LEVEL          Verbosity level: error, info, debug, trace (default: info)
  --xd N             Delete stale TMP staging after N days (default: 2)
  --bd N             Delete displaced files (BAK/) after N days (default: 90)
  --td N             Forget deletion records after N days (default: 180)

Quick start:
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

No file is ever destroyed \u{2014} displaced files go to .kitchensync/BAK/.";

#[derive(Debug)]
pub struct Cli {
    pub max_connections: usize,
    pub connect_timeout: u64,
    pub verbosity: String,
    pub staging_expiry_days: u64,
    pub backup_expiry_days: u64,
    pub tombstone_expiry_days: u64,
    pub peers: Vec<String>,
}

fn validation_error(msg: &str) -> ! {
    println!("Error: {}", msg);
    println!();
    println!("{}", HELP_TEXT);
    std::process::exit(1);
}

fn require_next_arg(args: &[String], i: usize, flag: &str) -> String {
    if i + 1 >= args.len() {
        validation_error(&format!("{} requires a value", flag));
    }
    args[i + 1].clone()
}

fn parse_positive_int(flag: &str, value: &str) -> usize {
    match value.parse::<usize>() {
        Ok(n) if n > 0 => n,
        _ => validation_error(&format!("{} must be a positive integer, got '{}'", flag, value)),
    }
}

/// Returns true if arg looks like an unknown flag rather than a subordinate peer.
/// Subordinate peers start with - followed by a path or URL (e.g., -/path, -./path,
/// -sftp://..., -[...], -c:\path). Flags start with - followed by short letters.
fn is_flag_like(arg: &str) -> bool {
    let rest = &arg[1..];
    // Subordinate peer patterns
    if rest.starts_with('/') || rest.starts_with('[') || rest.starts_with('.') {
        return false;
    }
    if rest.contains("://") {
        return false;
    }
    // Windows drive letter like -c:\path
    if rest.len() >= 2 {
        let bytes = rest.as_bytes();
        if bytes[0].is_ascii_alphabetic() && (bytes[1] == b':' || bytes[1] == b'\\') {
            // But only if it looks like a path, not a short flag
            // -c: or -c\ is a path, -c alone would be a flag
            return false;
        }
    }
    // If first char after - is not alphabetic, treat as peer
    let first = rest.chars().next().unwrap_or('0');
    first.is_ascii_alphabetic()
}

pub fn parse_args() -> Cli {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // No arguments → print help and exit 0
    if args.is_empty() {
        println!("{}", HELP_TEXT);
        std::process::exit(0);
    }

    // Check for help flags anywhere in args
    for arg in &args {
        if arg == "-h" || arg == "--help" || arg == "/?" {
            println!("{}", HELP_TEXT);
            std::process::exit(0);
        }
    }

    let mut max_connections: usize = 10;
    let mut connect_timeout: u64 = 30;
    let mut verbosity = String::from("info");
    let mut staging_expiry_days: u64 = 2;
    let mut backup_expiry_days: u64 = 90;
    let mut tombstone_expiry_days: u64 = 180;
    let mut peers: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--mc" => {
                let val = require_next_arg(&args, i, "--mc");
                max_connections = parse_positive_int("--mc", &val);
                i += 2;
            }
            "--ct" => {
                let val = require_next_arg(&args, i, "--ct");
                let n = parse_positive_int("--ct", &val);
                connect_timeout = n as u64;
                i += 2;
            }
            "-vl" => {
                let val = require_next_arg(&args, i, "-vl");
                match val.as_str() {
                    "error" | "info" | "debug" | "trace" => {
                        verbosity = val;
                    }
                    _ => validation_error(&format!(
                        "-vl must be one of error, info, debug, trace, got '{}'",
                        val
                    )),
                }
                i += 2;
            }
            "--xd" => {
                let val = require_next_arg(&args, i, "--xd");
                staging_expiry_days = parse_positive_int("--xd", &val) as u64;
                i += 2;
            }
            "--bd" => {
                let val = require_next_arg(&args, i, "--bd");
                backup_expiry_days = parse_positive_int("--bd", &val) as u64;
                i += 2;
            }
            "--td" => {
                let val = require_next_arg(&args, i, "--td");
                tombstone_expiry_days = parse_positive_int("--td", &val) as u64;
                i += 2;
            }
            "--" => {
                // End of options; remaining args are peers
                for j in (i + 1)..args.len() {
                    peers.push(args[j].clone());
                }
                break;
            }
            _ if arg.starts_with("--") => {
                validation_error(&format!("unrecognized option '{}'", arg));
            }
            _ if arg.starts_with('-') && arg.len() > 1 && is_flag_like(arg) => {
                validation_error(&format!("unrecognized option '{}'", arg));
            }
            _ => {
                peers.push(arg.clone());
                i += 1;
            }
        }
    }

    Cli {
        max_connections,
        connect_timeout,
        verbosity,
        staging_expiry_days,
        backup_expiry_days,
        tombstone_expiry_days,
        peers,
    }
}
