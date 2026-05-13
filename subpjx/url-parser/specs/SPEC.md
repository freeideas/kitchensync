# Tagged URL Group Parser

## Purpose

Parse a string conforming to a defined "tagged URL group" grammar into a structured description, and normalize URLs to a canonical identity. The library is a pure function over text — no filesystem access, no network access, no system calls. The grammar is defined entirely within this document; the parser's only external dependencies are URI standards.

## Grammar

A **tagged URL group** is one of:

- A single URL or local path, optionally preceded by a single-character **role tag**.
- A bracket group of comma-separated URLs — `[url1,url2,...]` — optionally preceded by a single-character role tag.

The role tag appears at most once, at the start of the expression. It is never attached to individual URLs inside a bracket group. Three role tags exist:

| Tag    | Label         |
|--------|---------------|
| `+`    | `Canon`       |
| `-`    | `Subordinate` |
| (none) | `Normal`      |

The labels are opaque outputs of parsing; the seed does not assign behavioural meaning to them beyond "three distinguishable tag values driven by this grammar."

Each URL inside the group is one of:

- A **bare path**: forward-slash- or backslash-delimited, optionally with a Windows drive letter (`c:/foo`, `c:\foo`, `./relative`, `/abs`). Backslashes are accepted on input and treated equivalently to forward slashes.
- A `file://` URI per RFC 8089.
- An `sftp://` URI per RFC 3986, with optional userinfo (`user` or `user:password`), optional port, and an absolute path.

Each URL inside the group may carry a query string with these parameters; unrecognized parameter names are rejected:

| Param | Required value form |
|-------|---------------------|
| `mc`  | Positive integer    |
| `ct`  | Positive integer    |
| `ka`  | Positive integer    |

The meaning of these parameters is opaque to the parser — it only validates syntax and exposes the parsed values to the caller.

## Output structure

`parse` yields a value with this shape (host-language records / sum types — no specific serialization):

```
TaggedGroup {
  role:  Canon | Subordinate | Normal
  urls:  List<ParsedUrl>    // at least one entry
}

ParsedUrl {
  scheme:    "file" | "sftp"
  user:      String?        // sftp only; null when no userinfo
  password:  String?        // sftp only; null when no password
  host:      String?        // sftp only
  port:      Integer?       // sftp only; null means "default" (22)
  path:      String         // absolute path, forward-slash-delimited
  query:     Map<String,String>   // recognised parameters from the original query
  identity:  String         // canonical identity (see Normalization)
}
```

Bare paths are converted into `file://` URIs before population. Relative bare paths are resolved against a caller-supplied current working directory (provided as an argument — the parser performs no system calls).

## Normalization

The `identity` field is computed for every URL using this procedure:

1. Lowercase the scheme.
2. For `sftp://` URLs, lowercase the host.
3. For `sftp://` URLs with no userinfo, insert a caller-supplied default username.
4. Remove the default port for the scheme (22 for `sftp`).
5. Collapse consecutive slashes in the path.
6. Remove any trailing slash from the path; do not reduce the path below `/`.
7. Percent-decode unreserved characters per RFC 3986 §2.3.
8. Drop the query string from `identity` (it is preserved separately on `ParsedUrl.query`).
9. For `file://` URIs, resolve relative paths to absolute paths against the caller-supplied current working directory.

Two URLs whose `identity` strings are equal name the same target.

## API surface

- `parse(text, cwd, default_user)` → `TaggedGroup` — parse one tagged URL group expression. `cwd` is a forward-slash-delimited absolute directory; relative bare paths are resolved against it. `default_user` is inserted for `sftp://` URLs that omit userinfo.
- `normalize(url, cwd, default_user)` → `String` — convenience: given a single URL with no role tag and no bracket group, return its canonical `identity`. Equivalent to `parse(url, cwd, default_user).urls[0].identity`.

Errors are reported through the host language's idiomatic mechanism. The parser rejects:

- Empty input.
- Multiple role tags.
- A bracket group that is not closed, that contains an empty URL, or that contains a role tag on an inner URL.
- Any URL with an unrecognized scheme.
- Any query parameter outside the allowed set, or any value that fails the parameter's required form.
- `sftp://` URLs without a host.
- `sftp://` URLs with a port outside `1..=65535`.

## Examples

- `+c:/photos` with `cwd=/home/u, default_user=ace` → role=`Canon`; one `ParsedUrl` with `scheme="file"`, `path="/c:/photos"`, `identity="file:///c:/photos"`.
- `./data` with `cwd=/home/u` → role=`Normal`; `scheme="file"`, `path="/home/u/data"`, `identity="file:///home/u/data"`.
- `[sftp://192.168.1.50/photos,sftp://nas.vpn/photos]` with `default_user=ace` → role=`Normal`; two `ParsedUrl`s with `scheme="sftp"`, `user="ace"`, host as given, identities `sftp://ace@192.168.1.50/photos` and `sftp://ace@nas.vpn/photos`.
- `"SFTP://Host:22/path/?mc=5"` with `default_user=ace` → role=`Normal`; identity `sftp://ace@host/path`; `query={"mc":"5"}`.
- `sftp://host//docs/` with `default_user=ace` → identity `sftp://ace@host/docs`.

## Anchoring

- URL/URI generic syntax: **RFC 3986** — scheme, authority (userinfo, host, port), path, query, percent-encoding rules, and unreserved-character set (§2.3).
- `file://` URI form: **RFC 8089**.
- `sftp://` URI: RFC 3986 generic-URI grammar applied to the conventional `sftp` scheme; userinfo (`user[:password]`) and host/port follow RFC 3986 §3.2.
- Bare paths, Windows drive letters, forward-slash-delimited path components: host-language string and filesystem-path primitives.
- Records, sum types, ordered lists, maps: host-language collection types.
