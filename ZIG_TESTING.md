# Zig Testing Strategy

This project adopts a streamlined, in-source testing approach that keeps test code close to the implementation for maximum maintainability and visibility, leveraging Zig's native testing features.

## Key Principles

### 1. Test Blocks Next to Implementation

- Test code is written directly in the same `.zig` file as the functions it verifies.
- Place `test` blocks immediately after the relevant function for clear association and easy maintenance.
- **Every non-trivial function should have a corresponding `_TEST_` block** (trivial functions like simple getters/setters may be skipped).

```zig
const std = @import("std");
const testing = std.testing;

pub fn add(a: i32, b: i32) i32 {
    return a + b;
}

test "add_TEST_" {
    try testing.expectEqual(@as(i32, 4), add(2, 2));
}
```

### 2. Test Naming Convention

- Use descriptive string labels for test blocks.
- For method-specific tests, use `"FunctionName_TEST_"` as the label.
- For broader module or "whole-class" tests, use `"__TEST__"`.

```zig
test "calculateSum_TEST_" {
    // Test implementation
}
test "__TEST__" {
    // Comprehensive module test
}
```

### 3. Whole-Module Testing

- Use a `test "__TEST__"` block to cover the main functionality of a module or group of related functions.
- This serves as a high-level integration test for the file.

### 4. Every Module Is Testable

- Ensure every `.zig` file contains at least one `test` block.
- Every non-trivial function should have its own test.
- Zig's test runner will automatically discover and execute all test blocks.

### 5. Project Entry Point

- To run all tests in a file:  
  `zig test path/to/your_module.zig`
- For multi-file projects, create a test runner that imports all modules, or run `zig test` on each file individually.

### 6. Test Execution & Reporting

- Zig automatically finds and runs all `test` blocks in declaration order.
- Output includes clear pass/fail status and error locations.

### 7. No Test File Pollution

- No need for separate test directories or files.
- Temporary or experimental tests can be placed in a `temp.zig` file or a `Temp` directory.

## Assertion Strategy

### Zig Built-In Assertions

- Use `std.testing.expect(condition)` for boolean conditions
- Use `std.testing.expectEqual(expected, actual)` for equality checks
- Use `std.testing.expectEqualStrings(expected, actual)` for string comparisons
- Use `std.testing.expectError(expected_error, actual)` for error checking

```zig
const std = @import("std");
const testing = std.testing;

test "example_TEST_" {
    try testing.expect(1 > 0); // Boolean condition
    try testing.expectEqual(@as(i32, 4), add(2, 2)); // Equality check
}
```

## Testing Philosophy

- Focus tests on essential behaviors and the "happy path".
- Avoid over-testing unlikely edge cases unless they represent critical logic.
- Maintain tests alongside code to simplify refactoring and improve code quality.

## Benefits

- **Proximity:** Tests are always next to the code they verify.
- **Visibility:** Easy to see what is covered.
- **Simplicity:** No external frameworks or dependencies required.
- **Discoverability:** Zig's test runner finds all tests by convention.
- **Maintainability:** Tests move with code during refactoring.
- **AI-Friendly:** Test blocks serve as usage examples for tools and code exploration.

## Example Zig File

```zig
const std = @import("std");
const testing = std.testing;

pub fn multiply(a: i32, b: i32) i32 {
    return a * b;
}

test "multiply_TEST_" {
    try testing.expectEqual(@as(i32, 12), multiply(3, 4));
    try testing.expectEqual(@as(i32, -1), multiply(-1, 1));
}

test "__TEST__" {
    // Comprehensive module test
    try testing.expectEqual(@as(i32, 50), multiply(10, 5));
}
```

## Running Tests

- Run all tests in a file:  
  `zig test path/to/your_module.zig`
- For temporary or experimental code, place tests in a `temp.zig` file or `Temp` directory.
