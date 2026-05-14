# Project failure: subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax

**Raised by:** Build
**Counter:** Build = 1/3
**Timestamp:** 2026-05-14-23-44-00-226

## Failure summary

build.py build returned 1

## Evidence

```
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:54: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:60: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:66: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:74: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:79: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:91: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:98: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:116: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:123: Unsupported escape sequence.
e: file:///C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/subpjx/gitignore-pattern-syntax/build/_aitc-deps.gradle.kts:3:148: Unsupported escape sequence.

FAILURE: Build failed with an exception.

* Where:
Script 'C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\build\_aitc-deps.gradle.kts' line: 3

* What went wrong:
Script compilation errors:

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                               ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                     ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                           ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                   ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                        ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                                    ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                                           ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                                                             ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                                                                    ^ Unsupported escape sequence.

  Line 3:     add("implementation", fileTree(mapOf("dir" to "C:\Users\human\Desktop\prjx\kitchensync\subpjx\gitignore-matcher\subpjx\gitignore-pattern-syntax\lib", "include" to "*.jar", "exclude" to "*_MCP.jar")))
                                                                                                                                                             ^ Unsupported escape sequence.

10 errors

* Try:
> Run with --stacktrace option to get the stack trace.
> Run with --info or --debug option to get more log output.
> Run with --scan to generate a Build Scan (Powered by Develocity).
> Get more help at https://help.gradle.org.

BUILD FAILED in 4m 18s
```
