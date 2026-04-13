# Peer Filesystem Interface

All sync logic operates through a single interface that both `file://` and `sftp://` implement. No protocol-specific code exists outside the interface implementations.

## Interface

| Operation                  | Description                                                       |
| -------------------------- | ----------------------------------------------------------------- |
| `ListDir(path)`            | List immediate children (name, isDir, modTime, byteSize). byteSize is file size for files, -1 for directories |
| `Stat(path)`               | Return modTime, byteSize, isDir; or "not found"                  |
| `ReadFile(path)` -> Reader | Open file for streaming read                                      |
| `WriteFile(path, Reader)`  | Create/overwrite file from stream, creating parent dirs as needed |
| `Rename(src, dst)`         | Same-filesystem rename (for TMP -> final swap and BAK displacement) |
| `DeleteFile(path)`         | Remove a file                                                     |
| `CreateDir(path)`          | Create directory (and parents as needed)                          |
| `DeleteDir(path)`          | Remove empty directory                                            |
| `SetModTime(path, time)`   | Set file/directory modification time                              |
| `GetPermissions(path)`     | Return file mode/permissions (platform-appropriate)               |
| `SetPermissions(path, mode)` | Set file mode/permissions (best-effort, ignore failures)        |

## Entry Filtering

`ListDir` returns only regular files and directories. Symbolic links, special files (devices, FIFOs, sockets), and any other non-regular entries are silently omitted. `Stat` returns "not found" for symlinks and special files.

## Error Types

All operations return the same error types regardless of transport: not found, permission denied, I/O error. Network failures surface as I/O errors.

## SFTP Implementation

SFTP connections must use standard OS hostname resolution. Bare hostnames like `localhost` must resolve correctly -- do not use numeric-only address parsing.

SSH authentication follows the standard sequence: SSH agent, then key files (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, etc.), then inline password if provided in the URL.

Host key verification uses the system's `~/.ssh/known_hosts` file. The implementation must handle the case where the server offers multiple key types -- constrain the handshake to negotiate only key types that `known_hosts` can verify for the target host.

## file:// Implementation

Local filesystem operations. Connection timeout (`--ct`) does not apply. On Windows, `file://` paths must be converted from URL form (`/c:/photos`) to OS-native form (`c:/photos`) for all filesystem calls.

## Permissions

Best-effort permission copying: on Unix-like systems, copy the file mode bits. On Windows, skip permission copying entirely (Windows uses ACLs which are not portable). Failures are logged at debug level and ignored.
