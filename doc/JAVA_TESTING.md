# Java Testing Strategy

This project uses a simple, home-grown testing approach that keeps test code close to the implementation for better maintainability and visibility.

## Key Principles

### 1. Test Methods Next to Implementation

Test code is located directly in the same class file as the methods being tested. This provides immediate visibility into which methods have test coverage and makes it easy to maintain tests alongside the code they verify.

### 2. Test Method Naming Convention

Test methods follow a specific naming pattern:
- Method name ends with `_TEST_`
- Must be `private static`
- Returns `boolean` (true for pass, false for fail)
- Optionally accepts a single `boolean findLineNumber` parameter
- Should include `@SuppressWarnings("unused")` annotation since they're called via reflection

Example:
```java
@SuppressWarnings("unused")
private static boolean methodName_TEST_(boolean findLineNumber) {
    if (findLineNumber) throw new RuntimeException();
    // Test implementation
    return true; // or false if test fails
}
```

The `findLineNumber` parameter serves a specific purpose: it allows the test framework to determine the line number where each test method is defined. This information is used to execute tests in the order they appear in the source code, which is what developers naturally expect when reading through a class file.

### 3. Whole-Class Testing

For testing an entire class, use the special method signature:
```java
@SuppressWarnings("unused")
private static boolean __TEST__(boolean findLineNumber) {
    // Test entire class functionality
    return true;
}
```

### 4. Every Class Should Be Testable

Every class should include a `main` method that runs its tests:
```java
public static void main(String[] args) {
    LibTest.testClass();
}
```

### 5. Entry Point Classes

For classes that serve as application entry points and need their own functional `main` method, use the `_TEST_` command-line argument convention:

```java
public static void main(String[] args) {
    if (args.length > 0 && "_TEST_".equals(args[0])) {
        LibTest.testClass();
        return;
    }
    // Regular main method implementation
}
```

## Test Execution

The `LibTest` class can be copied from the `doc/copy_src/jLib/` directory if it doesn't already exist in the project. The `LibTest.testClass()` method automatically:
1. Discovers all methods ending with `_TEST_` in the calling class
2. Determines line numbers by invoking tests with `findLineNumber=true`
3. Executes tests in order of their line numbers
4. Reports results with clear pass/fail status and error locations

### Running Tests

**IMPORTANT**: Never create separate test files or debug classes. Every Java class tests itself through its main method.

To run tests for a specific class:
```bash
# Using java.sh (recommended)
./java.sh http.HttpHeader

# Or directly with java
java -cp target/classes http.HttpHeader
```

To run ALL tests in the entire project:
```bash
# Using LibTest's main method (finds and runs every _TEST_ method in all classes)
java -cp "lib/*:target/classes" jLib.LibTest

# Or using the test script
./test.sh
```

The `LibTest.main()` method automatically discovers all Java files in the project, loads their classes, and runs all `_TEST_` methods found in each class. This provides a comprehensive test suite without needing external test runners or frameworks.

### No Test File Pollution

This testing philosophy means:
- **NO** separate test directories (src/test/java)
- **NO** temporary debug files (DebugTest.java, QuickTest.java, etc.)
- **RARE** test-only classes
- Every `.java` file is self-contained with its own tests
- If you need to debug something, add a `_TEST_` method to the relevant class

### Exception: temp Package

The `temp` package (`src/main/java/temp/`) is reserved for temporary test files:
- Developers can safely delete all files in this package at any time
- Use this for quick experiments or debugging specific issues
- Files in this package are not part of the production codebase
- These files should not be referenced by any production code

## Assertion Strategy

This project uses custom assertion methods instead of Java's built-in `assert`:
- **Use `LibTest.asrt(condition)`** - Throws AssertionError if condition is false
- **Use `LibTest.asrtEQ(expected, actual)`** - Throws AssertionError if values are not equal
- **Never use Java's `assert` statement** - It requires JVM flags and can be disabled

Best practices:
- Use `asrtEQ` for equality checks: `LibTest.asrtEQ(expected, actual)`
- Use `asrt` for boolean conditions: `LibTest.asrt(list.isEmpty())`
- Use `asrt` for null checks: `LibTest.asrt(obj != null)`
- Never use `asrt` with `==` for value equality - use `asrtEQ` instead

Benefits of custom assertions:
- Always run regardless of JVM configuration
- Provide consistent error messages
- Cannot be accidentally disabled in production

## Testing Philosophy

Focus testing on behaviors that are needed now, not what might be needed in the future:
- Test the happy path - components working correctly with valid inputs
- Avoid testing edge cases that may never occur in practice
- Don't test error handling unless it represents critical business logic
- Time spent testing hypothetical scenarios is better spent improving necessary functionality

## Benefits

- **Proximity**: Tests live next to the code they test
- **Visibility**: Easy to see which methods have test coverage
- **Simplicity**: No external testing framework dependencies
- **Discoverability**: Test methods are automatically found by convention
- **Maintainability**: Tests move with the code during refactoring
- **AI-Friendly**: Test methods serve as usage examples within the same file, enabling AI tools to understand APIs without loading multiple files, making better use of limited context windows