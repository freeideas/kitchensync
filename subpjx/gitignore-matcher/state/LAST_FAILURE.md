# Project failure: subpjx/gitignore-matcher

**Raised by:** RunTest
**Counter:** RunTest = 44/3
**Timestamp:** 2026-05-12-19-43-08-848

## Failure summary

Test `02_match-stack` failed against the current release.

## Evidence

```
Traceback (most recent call last):
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/tests/02_match-stack.py", line 202, in <module>
    if __name__ == "__main__":
                 ^^^^^^
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/tests/02_match-stack.py", line 119, in main
    # 02.2 — no pattern matches → NotIgnored
        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/tests/02_match-stack.py", line 82, in _match
    def _match(sock, stack, relative_path, is_directory, rpc_id):
               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/home/ace/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/tests/02_match-stack.py", line 69, in _call
    raise RuntimeError(f"tool {tool!r} error: {r['error']}")
RuntimeError: tool 'match' error: {'code': -32000, 'message': 'invalid argument: stack entry patterns required'}
```
