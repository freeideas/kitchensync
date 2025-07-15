## Task: Implement This Project

### Phase 0: Clean Project Directory
1. Delete every file in the project directory except for the .md files in the root of the project, and the doc subdirectory (if any).

### Phase 1: Incremental Implementation
1. Read all .md files in the current directory to understand the project requirements
2. Implement the project incrementally:
   - Write each module/component one at a time
   - Run tests for each module immediately after writing it
   - Fix any test failures before proceeding to the next module
3. If you discover design defects during implementation:
   - Update DESIGN.md with the necessary corrections
   - Update other .md files if affected

### Phase 2: Integration Testing
1. After all modules are complete, run the end-to-end test
2. Fix any issues until the end-to-end test passes
3. Verify the complete system works as specified

### Phase 3: Documentation Report
Create report.txt containing ONLY implementation-specific findings:

**STRICT REQUIREMENTS:**
- NO feature suggestions or enhancements
- NO explanations of programming concepts
- Focus ONLY on lessons learned from this specific project

**REQUIRED SECTIONS:**

1. **Implementation Obstacles Encountered**
   Document specific technical challenges unique to this project:
   - Which standard library functions behaved unexpectedly and how
   - Specific bugs found and their root causes
   - Complex implementation logic that required multiple attempts
   - Unexpected edge cases in operations
   - Component interactions that caused issues

2. **Design Documentation Gaps**
   List what was missing or ambiguous in the design:
   - Unspecified behavior for specific scenarios
   - Missing details about error handling requirements
   - Ambiguous algorithm specifications
   - Platform-specific requirements that weren't documented

3. **Concrete Additions for DESIGN.md**
   Provide specific implementation details to add:
   - Exact API/library function calls that work correctly for each operation
   - Specific error types to handle and how
   - The working algorithm implementation details
   - Memory allocation patterns where relevant
   - Precise data format and structure specifications

4. **Documentation Updates Made**
   List any changes made to .md files during implementation:
   - Which files were updated and why
   - What corrections or clarifications were added

**OUTPUT FORMAT:**
Write findings as specific, actionable documentation additions that would prevent another developer from encountering the same issues. Each point should be a concrete detail about THIS project's implementation, not general programming advice.
