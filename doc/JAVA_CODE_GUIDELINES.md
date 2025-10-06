# Code Guidelines

**Note: These guidelines apply to all programming languages. While examples are shown in specific languages, the core philosophies should be followed regardless of the language being used.**

## Core Philosophy

### Brevity Above All

Fewer lines of code to achieve the same result beats almost every other concern:

- **Clarity**: A few lines of code is clearer than a lot of code
- **Performance**: A few lines of code will often run faster than a lot of code
- **Optimization**: When brevity doesn't improve performance directly, it's easier to optimize a few lines than a large body of code


### Only Write What's Needed

Don't write any code unless there is reason to believe it will be needed. This includes utility methods, helper functions, getters/setters, or any "just in case" code.

### Prefer Early Returns and Breaks

Always make code shorter and flatter when possible instead of nesting blocks:

```python
# AVOID: Deeply nested code
def process_item(item):
    if item is not None:
        if item.is_valid():
            if item.can_process():
                # Process the item
                return True
    return False

# BETTER: Flat code with early returns
def process_item(item):
    if item is None:
        return False
    if not item.is_valid():
        return False
    if not item.can_process():
        return False
    
    # Process the item
    return True
```

Similarly for loops:

```javascript
// AVOID: Nested conditions inside loops
while (true) {
    if (someCondition) {
        doSomeThings();
        doMoreThings();
    } else {
        break;
    }
}

// BETTER: Early break to flatten structure
while (true) {
    if (!someCondition) break;
    doSomeThings();
    doMoreThings();
}
```

### On Exception/Error Handling

- Do not catch exceptions just to re-throw them
- Let errors bubble up naturally
- Do not clutter code with unnecessary try-catch blocks
- Only catch exceptions when:
  - They need to be part of a return value
  - You're actually handling the error in a meaningful way
  - The language/framework requires explicit error handling

### On Warnings

- Fix all compiler, linter, and runtime warnings
- Warnings can be distracting from real problems
- Clean code compiles/runs without warnings
- If a warning can't be fixed, explicitly suppress it with appropriate directives and document why

### On Comments

- Comments are almost always lies
  - If they aren't lies right now, they will be after changes happen
  - Writing comments tempts developers to write code that is difficult to understand
  - If code isn't self-explanatory, improve the code rather than explaining it with comments
  - Better code usually means fewer lines of code

#### When Comments Are Acceptable:
- **Surprises**: Document genuinely surprising behavior that someone reading the code might not expect
- **Class/Module Purpose**: A brief comment at the top of a class or module explaining its purpose and what makes it valuable
- **Non-obvious Business Logic**: When the code correctly implements counter-intuitive requirements

Example of a good comment:
```python
"""
This module implements a lightweight tree structure that can represent
any part of any tree-like data as a node.
"""
```

### On Console Output

- Perfect code doesn't log anything unless something unexpected happens
- No console prints (print(), console.log(), etc.) except where the application specifically requires output
- Debug logs are almost always useless because it takes too much time to read them
- Use proper logging frameworks when logging is necessary, not console output

### On File Management

- Keep the project directory pristine - a clean workspace leads to clearer thinking and easier navigation
- All temporary files must be created in the system's designated temporary directory (e.g., /tmp on Unix, %TEMP% on Windows)
- Never create test files, scratch files, or temporary outputs in the project directory
- The only files in the project should be essential source code, configuration, and documentation that serves the project's purpose
- When testing requires file creation, use appropriate temporary directory utilities provided by your language (tempfile in Python, mktemp in bash, etc.)
- Clean up temporary files when done - though using system temp directories ensures automatic cleanup on reboot

### On Testing Philosophy

- Tests should focus on verifying that components behave correctly with valid inputs
- Testing error handling and edge cases is usually not valuable - focus on the happy path
- A component that works correctly with proper inputs is far more important than one that gracefully handles invalid inputs
- Time spent testing error conditions is better spent making the component work better with correct inputs
- Exception: Test error conditions only when they represent important business logic or security boundaries
- For Java projects, see JAVA_TESTING.md for the specific testing strategy used in this codebase

## Spacing

### Blank Lines
- Use 3 blank lines:
  - Between methods or function defs
  - Before the first method and after the last method
  - Between imports and class declaration
  - EXCEPTION: anonymous inner classes have no blank lines
  - EXCEPTION: inner classes and private classes have one blank line between methods

- Use 0 blank lines:
  - Between closely related methods (same name with different args, getter/setter pairs, constructors)
  - Between a method and its test method
  - Within method bodies
  - Within inner classes

## Code Structure

### Single Statement Blocks
- For single statement blocks, put statement on same line as control structure
- Don't use braces for single statement blocks (except try/catch/finally)
```java
if (condition) return;
while (condition) callMethod();
for (int i = 0; i < 10; i++) callMethod();
```

### Methods
- Methods with single-line bodies should be on same line as signature:
```java
public void methodName() { callMethod(); }
```

### try/catch/finally blocks
- try/catch/finally with single statement bodies should be on same line as try/catch/finally keyword:
```java
try { callMethod(); }
catch (Exception e) { logger.error(e); }
finally { cleanup(); }
```

## Line Length
- Maximum line length is 120 characters; break lines when they would exceed this limit

## Comments
- Remove EVERY comment unless it explains:
  1. Why this code is needed (it should not exist unless it is needed)
  2. Surprising behavior
  3. Something that won't be obvious to someone who has already read the code

### Examples of Acceptable Comments:
```java
// NOTE: We multiply by 1.5 here because the API returns values in a different unit
double adjustedValue = apiValue * 1.5;

// NOTE: This sleep is required because the external service has a rate limit
Thread.sleep(100);

/**
 * This class is needed because fetching the user data is expensive and it is used frequently,
 * and tests without the cache time-out.
 */
public class UserCache {
}
```