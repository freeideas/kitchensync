# Code Guidelines for Zig Development

**Note: While these guidelines are written with Zig in mind, the core philosophies apply to all programming languages and should be followed regardless of the language being used.**

## Core Philosophy

### Brevity Above All

Fewer lines of code to achieve the same result beats almost every other concern:

- **Clarity**: A few lines of code is clearer than a lot of code
- **Performance**: A few lines of code will often run faster than a lot of code
- **Optimization**: When brevity doesn't improve performance directly, it's easier to optimize a few lines than a large body of code


### Only Write What's Needed

Don't write any code unless there is reason to believe it will be needed. This means, don't write utility functions, helper structs, getters/setters, or any "just in case" code, unless you know it is necessary.

### Prefer Early Returns and Breaks

Always make code shorter and flatter when possible instead of nesting blocks:

```zig
// AVOID: Deeply nested code
fn processItem(item: ?*Item) bool {
    if (item) |i| {
        if (i.isValid()) {
            if (i.canProcess()) {
                // Process the item
                return true;
            }
        }
    }
    return false;
}

// BETTER: Flat code with early returns
fn processItem(item: ?*Item) bool {
    const i = item orelse return false;
    if (!i.isValid()) return false;
    if (!i.canProcess()) return false;    
    // Process the item
    return true;
}
```

Similarly for loops:

```zig
// AVOID: Nested conditions inside loops
while (true) {
    if (someCondition()) {
        doSomeThings();
        doMoreThings();
    } else {
        break;
    }
}

// BETTER: Early break to flatten structure
while (true) {
    if (!someCondition()) break;
    doSomeThings();
    doMoreThings();
}
```

### On Error Handling

- Use Zig's error unions properly - don't catch errors just to re-propagate them
- Let errors bubble up naturally with `try`
- Do not clutter code with unnecessary error handling
- Only catch errors when:
  - They need to be transformed into a different error or return value
  - You're actually handling the error in a meaningful way
  - You need to add cleanup or logging

```zig
// AVOID: Unnecessary error handling
fn doSomething() !void {
    someFunction() catch |err| {
        return err;
    };
}

// BETTER: Let errors propagate naturally
fn doSomething() !void {
    try someFunction();
}
```

### On Compiler Warnings

- Fix all compiler warnings
- Warnings can be distracting from real problems
- Clean code compiles without warnings
- Use explicit type conversions when needed (e.g., `@intCast`, `@floatCast`)
- If a warning must be suppressed, document why with a comment

### On Comments

- Comments are almost always lies
  - If they aren't lies right now, they will be after changes happen
  - Writing comments tempts developers to write code that is difficult to understand
  - If code isn't self-explanatory, improve the code rather than explaining it with comments
  - Excellent code requires very few comments
  - NEVER write comments that are obvious to someone who can read the code (e.g. arg types and return types, etc.)

#### When Comments Are Acceptable:
- **Surprises**: Document genuinely surprising behavior that someone reading the code might not expect
- **Struct/Module Purpose**: A brief comment at the top of a struct or module explaining its purpose and what makes it valuable
- **Non-obvious Business Logic**: When the code correctly implements counter-intuitive requirements

Example of a good comment:
```zig
/// This allocator is unusual because it allocates
/// memory in O(1) time but cannot free individual allocations.
const ArenaAllocator = struct {
    // ...
};
```

### On Debug Output

- Perfect code doesn't print anything unless something unexpected happens
- No debug prints (std.debug.print, std.log, etc.) except where the application specifically requires output
- Debug logs are almost always useless because it takes too much time to read them
- Use std.log scoped logging when logging is necessary, not raw debug prints
- Remove all debug prints before committing code

### On File Management

- Keep the project directory pristine - a clean workspace leads to clearer thinking and easier navigation
- All temporary files must be created in the system's designated temporary directory
- Never create test files, scratch files, or temporary outputs in the project directory
- The only files in the project should be essential source code, build.zig, and documentation that serves the project's purpose
- Clean up temporary files when done using `defer`

## Spacing

### Blank Lines
- Use 3 blank lines:
  - Between top-level declarations (functions, structs, const declarations)
  - Before the first declaration and after the last declaration
  - Between imports and declarations
  - EXCEPTION: nested structs have no blank lines
  - EXCEPTION: closely related small functions have one blank line between them

- Use 0 blank lines:
  - Between closely related functions (same name with different parameters, init/deinit pairs)
  - Within function bodies
  - Within struct definitions

## Code Structure

### Zig Project Organization
- Follow standard Zig project structure:
  - `src/` - All source files
  - `src/main.zig` - Application entry point
  - `build.zig` - Build configuration
  - Use descriptive subdirectories within `src/` for larger projects:
    - `core/` - Core functionality
    - `utils/` - Utility functions
- This structure keeps the project root clean and follows Zig conventions

### Single Statement Blocks
- For single statement blocks, put statement on same line as control structure when it fits
- Zig requires braces for all blocks, but keep them compact:
```zig
if (condition) { return; }
while (condition) { callMethod(); }
for (items) |item| { processItem(item); }
```

### Functions
- Functions with single-line bodies should be on same line as signature when it fits:
```zig
fn getValue(self: *Self) u32 { return self.value; }
fn setValue(self: *Self, value: u32) void { self.value = value; }
```

### Error Handling Blocks
- Single statement error handling should be compact:
```zig
const result = doSomething() catch { return error.Failed; };
const value = getValue() catch |err| { log.err("Failed: {}", .{err}); return; };
defer cleanup() catch {}; // Ignore errors in cleanup
```

## Line Length
- Maximum line length is 120 characters; break lines when they would exceed this limit

## Comments
- Remove EVERY comment unless it explains:
  1. Why this code is needed (it should not exist unless it is needed)
  2. Surprising behavior
  3. Something that won't be obvious to someone who has already read the code

### Examples of Acceptable Comments:
```zig
// NOTE: We multiply by 1.5 here because the API returns values in a different unit
const adjusted_value = api_value * 1.5;

// NOTE: This sleep is required because the external service has a rate limit
std.time.sleep(100 * std.time.ns_per_ms);

const UserCache = struct {
    // NOTE: This cache is needed because tests show that this data is fetched frequently and is expensive to retrieve
    // NOTE: Using HashMap instead of ArrayHashMap because we need stable pointers
    map: std.hash_map.HashMap(u64, User, std.hash_map.AutoContext(u64), 80),
};
```

## Zig-Specific Guidelines

### Memory Management
- Always use `defer` for cleanup immediately after allocation
- Prefer arena allocators for temporary allocations
- Document allocator requirements in function signatures

### Comptime
- Use comptime to eliminate runtime overhead when possible
- But don't overuse it - runtime code is often clearer

### Error Sets
- Use inferred error sets unless you need a specific interface
- Don't create unnecessary error types

### Optionals and Error Unions
- Use optionals for values that might not exist
- Use error unions for operations that might fail
- Don't mix the two unnecessarily

Remember: The goal is always to write the minimal amount of code that correctly solves the problem.
