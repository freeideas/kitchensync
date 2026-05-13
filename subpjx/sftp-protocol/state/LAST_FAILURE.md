# Project failure: subpjx/sftp-protocol

**Raised by:** RunTest
**Counter:** RunTest = 2/3
**Timestamp:** 2026-05-12-22-51-22-307

## Failure summary

Test `02_file-ops-listing-stat` failed against the current release.

## Evidence

```
Installed 7 packages in 25ms
Traceback (most recent call last):
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/sftp-protocol/tests/02_file-ops-listing-stat.py", line 425, in <module>
    sys.exit(main())
             ^^^^^^
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/sftp-protocol/tests/02_file-ops-listing-stat.py", line 193, in main
    had_known_hosts = KNOWN_HOSTS.exists()
                      ^^^^^^^^^^^
NameError: name 'KNOWN_HOSTS' is not defined
```
