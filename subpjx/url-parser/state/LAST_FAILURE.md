# Project failure: subpjx/url-parser

**Raised by:** RunTest
**Counter:** RunTest = 32/3
**Timestamp:** 2026-05-12-18-41-55-062

## Failure summary

Test `03_identity-normalization` failed against the current release.

## Evidence

```
[03.12] scheme lowercase: 'sftp://ace@host/path'
[03.13] host lowercase: 'sftp://ace@host/path'
[03.14] default_user inserted: 'sftp://ace@host/path'
[03.15] default port omitted: 'sftp://ace@host/path'
[03.16] consecutive slashes collapsed: 'sftp://ace@host/docs'
[03.17a] trailing slash removed: 'sftp://ace@host/path'
[03.17b] root slash preserved: 'sftp://ace@host/'
[03.18] percent-decode unreserved: 'sftp://ace@host/Apath'
[03.19] query excluded: identity='sftp://ace@host/path' query={'mc': '5'}
[03.20] relative path resolved: no urls in result; r={'id': 10.0, 'jsonrpc': '2.0', 'result': {'content': [{'text': '{"role":"Normal","urls":[{"identity":"file:///home/u/data","path":"/home/u/data","query":{},"scheme":"file"}]}', 'type': 'text'}]}}

FAILURES:
  - 03.20: parse returned no urls
```
