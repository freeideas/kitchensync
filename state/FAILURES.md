---
## 2026-05-17 22:24:13 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `03_sftp-pool` failed against the current release.

**Evidence:**

```
PASS: 03.63: file:// sync with pool flags exits 0
PASS: 03.63: file content synced correctly despite pool flags
PASS: 03.62: run exits within 20s when ct=3 (took 3.8s; server never responds)
PASS: 03.62: run exits non-zero when SSH handshake times out with no fallback
PASS: 03.59: per-URL ct=3 honored over global ct=60 (took 3.8s)
FAIL: 03.100: SFTP sync on explicit non-default port 60630 exits 0
FAIL: 03.100: file transferred over SFTP on non-default port
FAIL: 03.58: second SFTP URL to same user@host with different path exits 0
FAIL: 03.58: file transferred via second URL sharing same pool endpoint
FAIL: 03.96: sync to port 60649 (pool A) exits 0
FAIL: 03.96: file present on server A
FAIL: 03.96: sync to port 60651 (separate pool B) exits 0
FAIL: 03.96: file present on server B (different pool identity from A)
FAIL: 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
FAIL: 03.60+03.101: all 6 files present after sync with mc=1 (got 0)

10 FAILURE(S):
  - 03.100: SFTP sync on explicit non-default port 60630 exits 0
  - 03.100: file transferred over SFTP on non-default port
  - 03.58: second SFTP URL to same user@host with different path exits 0
  - 03.58: file transferred via second URL sharing same pool endpoint
  - 03.96: sync to port 60649 (pool A) exits 0
  - 03.96: file present on server A
  - 03.96: sync to port 60651 (separate pool B) exits 0
  - 03.96: file present on server B (different pool identity from A)
  - 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
  - 03.60+03.101: all 6 files present after sync with mc=1 (got 0)
```

---
## 2026-05-17 22:24:21 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `04_retention` failed against the current release.

**Evidence:**

```
FAIL
- 04.3 multi-tree walk should remove stale root .kitchensync/BAK timestamp directory
- 04.4 multi-tree walk should remove stale root .kitchensync/TMP timestamp directory
- 04.3 multi-tree walk should remove stale nested .kitchensync/BAK timestamp directory
- 04.4 multi-tree walk should remove stale nested .kitchensync/TMP timestamp directory
```

---
## 2026-05-17 22:25:05 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `01_cli-grammar` failed against the current release.

**Evidence:**

```
Exception (server): Error reading SSH protocol banner[WinError 10053] An established connection was aborted by the software in your host machine
Traceback (most recent call last):
  File "C:\Users\human\AppData\Local\uv\cache\environments-v2\01-cli-grammar-c5d420da13df1c1e\Lib\site-packages\paramiko\transport.py", line 2213, in _check_banner
    buf = self.packetizer.readline(timeout)
  File "C:\Users\human\AppData\Local\uv\cache\environments-v2\01-cli-grammar-c5d420da13df1c1e\Lib\site-packages\paramiko\packet.py", line 395, in readline
    buf += self._read_timeout(timeout)
           ~~~~~~~~~~~~~~~~~~^^^^^^^^^
  File "C:\Users\human\AppData\Local\uv\cache\environments-v2\01-cli-grammar-c5d420da13df1c1e\Lib\site-packages\paramiko\packet.py", line 663, in _read_timeout
    x = self.__socket.recv(128)
ConnectionAbortedError: [WinError 10053] An established connection was aborted by the software in your host machine

During handling of the above exception, another exception occurred:

Traceback (most recent call last):
  File "C:\Users\human\AppData\Local\uv\cache\environments-v2\01-cli-grammar-c5d420da13df1c1e\Lib\site-packages\paramiko\transport.py", line 2029, in run
    self._check_banner()
    ~~~~~~~~~~~~~~~~~~^^
  File "C:\Users\human\AppData\Local\uv\cache\environments-v2\01-cli-grammar-c5d420da13df1c1e\Lib\site-packages\paramiko\transport.py", line 2217, in _check_banner
    raise SSHException(
        "Error reading SSH protocol banner" + str(e)
    )
paramiko.ssh_exception.SSHException: Error reading SSH protocol banner[WinError 10053] An established connection was aborted by the software in your host machine

FAILURES:

- 01.24: default --mc SFTP sync failed:
exit=1
stdout:
unreachable peer URL: sftp://testuser@127.0.0.1:60673/mc-dst (host key rejected)
unreachable peer: sftp://testuser@127.0.0.1:60673/mc-dst

stderr:


- 01.24: trace output should include pool stats (connections=N/M); got:
'unreachable peer URL: sftp://testuser@127.0.0.1:60673/mc-dst (host key rejected)\nunreachable peer: sftp://testuser@127.0.0.1:60673/mc-dst\n'

- 01.32: omitting --xd should remove TMP dirs older than 2 days

- 01.33: omitting --bd should remove BAK dirs older than 90 days
```

---
## 2026-05-17 22:41:17 -- RunTest

**Counter:** 2 / 3 (RunTest)

**Summary:** Test `03_sftp-pool` failed against the current release.

**Evidence:**

```
PASS: 03.63: file:// sync with pool flags exits 0
PASS: 03.63: file content synced correctly despite pool flags
PASS: 03.62: run exits within 20s when ct=3 (took 3.8s; server never responds)
PASS: 03.62: run exits non-zero when SSH handshake times out with no fallback
PASS: 03.59: per-URL ct=3 honored over global ct=60 (took 3.8s)
FAIL: 03.100: SFTP sync on explicit non-default port 61264 exits 0
FAIL: 03.100: file transferred over SFTP on non-default port
FAIL: 03.58: second SFTP URL to same user@host with different path exits 0
FAIL: 03.58: file transferred via second URL sharing same pool endpoint
FAIL: 03.96: sync to port 61274 (pool A) exits 0
FAIL: 03.96: file present on server A
FAIL: 03.96: sync to port 61275 (separate pool B) exits 0
FAIL: 03.96: file present on server B (different pool identity from A)
FAIL: 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
FAIL: 03.60+03.101: all 6 files present after sync with mc=1 (got 0)

10 FAILURE(S):
  - 03.100: SFTP sync on explicit non-default port 61264 exits 0
  - 03.100: file transferred over SFTP on non-default port
  - 03.58: second SFTP URL to same user@host with different path exits 0
  - 03.58: file transferred via second URL sharing same pool endpoint
  - 03.96: sync to port 61274 (pool A) exits 0
  - 03.96: file present on server A
  - 03.96: sync to port 61275 (separate pool B) exits 0
  - 03.96: file present on server B (different pool identity from A)
  - 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
  - 03.60+03.101: all 6 files present after sync with mc=1 (got 0)
```

---
## 2026-05-17 22:42:25 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `03_parallel-listing` failed against the current release.

**Evidence:**

```
Command: C:\Users\human\Desktop\prjx\kitchensync\tools\compiler\jdk\bin\java.exe -jar C:\Users\human\Desktop\prjx\kitchensync\released\kitchensync.jar +C:\Users\human\AppData\Local\Temp\ks_test03_dvlykp81\src sftp://ks_test_user:ks_test_pw@127.0.0.1:61453/ sftp://ks_test_user:ks_test_pw@127.0.0.1:61454/ sftp://ks_test_user:ks_test_pw@127.0.0.1:61455/
Expecting elapsed < 3.1s (sequential would be ~4.5s)
Exit code : 1
Elapsed   : 2.38s
stdout:
unreachable peer URL: sftp://ks_test_user:ks_test_pw@127.0.0.1:61455/ (host key rejected)
unreachable peer URL: sftp://ks_test_user:ks_test_pw@127.0.0.1:61453/ (host key rejected)
unreachable peer URL: sftp://ks_test_user:ks_test_pw@127.0.0.1:61454/ (host key rejected)
unreachable peer: sftp://ks_test_user:ks_test_pw@127.0.0.1:61454/
unreachable peer: sftp://ks_test_user:ks_test_pw@127.0.0.1:61453/
unreachable peer: sftp://ks_test_user:ks_test_pw@127.0.0.1:61455/

PASS: elapsed 2.38s < 3.15s -- listings appear concurrent
FAIL: kitchensync exited 1, expected 0
```

---
## 2026-05-17 22:45:21 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `03_logging` failed against the current release.

**Evidence:**

```
FAILURES:
- sftp info sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/info (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/info\n'
- 03.105: info verbosity must include info-level C progress lines
- sftp debug sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/debug (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/debug\n'
- 03.105: debug verbosity must include info-level C progress lines
- sftp error sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/error (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/error\n'
- sftp trace sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/trace (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:61629/tmp/testks/ks03_logging_1355c721c16b4d5cac5e9fd897dfe97b/trace\n'
- 03.105: trace verbosity must include info-level C progress lines
- 03.82: trace verbosity must emit SFTP pool acquire/release events
- 03.109: trace output must include an SFTP pool acquire line with an active connection
- 03.109: trace output must include an SFTP pool release line returning to zero active connections
```

---
## 2026-05-17 22:47:48 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `04_error-handling` failed against the current release.

**Evidence:**

```
Installed 6 packages in 200ms
Socket exception: An existing connection was forcibly closed by the remote host (10054)
Traceback (most recent call last):
  File "C:\Users\human\Desktop\prjx\kitchensync\tests\04_error-handling.py", line 608, in <module>
    raise SystemExit(main())
                     ~~~~^^
  File "C:\Users\human\Desktop\prjx\kitchensync\tests\04_error-handling.py", line 590, in main
    check_snapshot_download_failure(work, checks)
    ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~^^^^^^^^^^^^^^
  File "C:\Users\human\Desktop\prjx\kitchensync\tests\04_error-handling.py", line 419, in check_snapshot_download_failure
    before = snapshot.read_bytes()
  File "C:\Users\human\AppData\Roaming\uv\python\cpython-3.13.6-windows-x86_64-none\Lib\pathlib\_abc.py", line 625, in read_bytes
    with self.open(mode='rb') as f:
         ~~~~~~~~~^^^^^^^^^^^
  File "C:\Users\human\AppData\Roaming\uv\python\cpython-3.13.6-windows-x86_64-none\Lib\pathlib\_local.py", line 537, in open
    return io.open(self, mode, buffering, encoding, errors, newline)
           ~~~~~~~^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
FileNotFoundError: [Errno 2] No such file or directory: 'C:\\Users\\human\\Desktop\\prjx\\kitchensync\\tests\\.tmp\\04_error_handling_0n1qdlk6\\snapshot-download-sftp-root\\bad-download\\.kitchensync\\snapshot.db'
```

---
## 2026-05-17 23:01:17 -- RunTest

**Counter:** 3 / 3 (RunTest)

**Summary:** Test `03_sftp-pool` failed against the current release.

**Evidence:**

```
PASS: 03.63: file:// sync with pool flags exits 0
PASS: 03.63: file content synced correctly despite pool flags
PASS: 03.62: run exits within 20s when ct=3 (took 3.8s; server never responds)
PASS: 03.62: run exits non-zero when SSH handshake times out with no fallback
PASS: 03.59: per-URL ct=3 honored over global ct=60 (took 3.7s)
FAIL: 03.100: SFTP sync on explicit non-default port 62626 exits 0
FAIL: 03.100: file transferred over SFTP on non-default port
FAIL: 03.58: second SFTP URL to same user@host with different path exits 0
FAIL: 03.58: file transferred via second URL sharing same pool endpoint
FAIL: 03.96: sync to port 62629 (pool A) exits 0
FAIL: 03.96: file present on server A
FAIL: 03.96: sync to port 62630 (separate pool B) exits 0
FAIL: 03.96: file present on server B (different pool identity from A)
FAIL: 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
FAIL: 03.60+03.101: all 6 files present after sync with mc=1 (got 0)

10 FAILURE(S):
  - 03.100: SFTP sync on explicit non-default port 62626 exits 0
  - 03.100: file transferred over SFTP on non-default port
  - 03.58: second SFTP URL to same user@host with different path exits 0
  - 03.58: file transferred via second URL sharing same pool endpoint
  - 03.96: sync to port 62629 (pool A) exits 0
  - 03.96: file present on server A
  - 03.96: sync to port 62630 (separate pool B) exits 0
  - 03.96: file present on server B (different pool identity from A)
  - 03.60: sync with mc=1 exits 0 (pool cap serializes, does not drop transfers)
  - 03.60+03.101: all 6 files present after sync with mc=1 (got 0)
```

---
## 2026-05-17 23:08:59 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `04_retention` failed against the current release.

**Evidence:**

```
FAIL
- retention sync should exit 0; got exit=1
stdout:
First sync? Mark the authoritative peer with a leading +

stderr:

- 04.3 multi-tree walk should remove stale root .kitchensync/BAK timestamp directory
- 04.4 multi-tree walk should remove stale root .kitchensync/TMP timestamp directory
- 04.3 multi-tree walk should remove stale nested .kitchensync/BAK timestamp directory
- 04.4 multi-tree walk should remove stale nested .kitchensync/TMP timestamp directory
```

---
## 2026-05-17 23:13:46 -- RunTest

**Counter:** 2 / 3 (RunTest)

**Summary:** Test `03_logging` failed against the current release.

**Evidence:**

```
FAILURES:
- sftp info sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/info (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/info\n'
- 03.105: info verbosity must include info-level C progress lines
- sftp debug sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/debug (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/debug\n'
- 03.105: debug verbosity must include info-level C progress lines
- sftp error sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/error (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/error\n'
- sftp trace sync exited 1: stdout='unreachable peer URL: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/trace (connection timeout)\nunreachable peer: sftp://ace:pw@127.0.0.1:63206/tmp/testks/ks03_logging_085b0ef0ece14206b41d1e679eeb36b8/trace\n'
- 03.105: trace verbosity must include info-level C progress lines
- 03.82: trace verbosity must emit SFTP pool acquire/release events
- 03.109: trace output must include an SFTP pool acquire line with an active connection
- 03.109: trace output must include an SFTP pool release line returning to zero active connections
```

---
## 2026-05-17 23:15:23 -- RunTest

**Counter:** 1 / 3 (RunTest)

**Summary:** Test `03_sftp-pool` failed against the current release.

**Evidence:**

```
PASS: 03.63: file:// sync with pool flags exits 0
PASS: 03.63: file content synced correctly despite pool flags
PASS: 03.62: run succeeds through fallback after timed-out SFTP URL
PASS: 03.62: ct=2 bounds the failed handshake before fallback (took 3.1s)
PASS: 03.62: next fallback URL receives the file
PASS: 03.59: timed-out SFTP URL without fallback fails the run
PASS: 03.59: per-URL ct=2 is honored over global ct=60 (took 2.7s)
PASS: 03.58+03.60: shared endpoint sync exits 0
FAIL: 03.58+03.97+03.107: shared endpoint obeys first mc=1 setting (max active writes 0)
PASS: 03.60: callers waiting on mc=1 eventually transfer all files
PASS: 03.61: sync with reusable idle SFTP connection exits 0
FAIL: 03.61: all payload writes reused one transfer connection within ka (used 0)
PASS: 03.96+03.101: sync to two SFTP ports exits 0
PASS: 03.100: explicit non-default port A receives files
PASS: 03.100: explicit non-default port B receives files
FAIL: 03.96+03.101: different port pools allow destination writes to overlap
PASS: 03.101: sync with mc=2 exits 0
FAIL: 03.101: endpoint uses available mc=2 transfer capacity (max active writes 0)
PASS: 03.64+03.112: SFTP-to-SFTP sync with mc=1 exits 0
FAIL: 03.64: source pool connection is used for payload reads
FAIL: 03.64: destination pool connection is used for payload writes
PASS: 03.64: payload transfers complete and connections are returned for later work

6 FAILURE(S):
  - 03.58+03.97+03.107: shared endpoint obeys first mc=1 setting (max active writes 0)
  - 03.61: all payload writes reused one transfer connection within ka (used 0)
  - 03.96+03.101: different port pools allow destination writes to overlap
  - 03.101: endpoint uses available mc=2 transfer capacity (max active writes 0)
  - 03.64: source pool connection is used for payload reads
  - 03.64: destination pool connection is used for payload writes
```

