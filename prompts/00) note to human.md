Some agentic coders are very good at writing code, but they will fail if they are asked to do too much or if your project has subtle design flaws.
Following this process will help you find and correct design flaws. Approximately 100% of software coding projects have design flaws.
This process will also attempt to give the agent small tasks, BUT:

=== YOU NEED TO BE SURE YOUR PROJECTS ARE BROKEN DOWN INTO SMALL PARTS THAT HAVE CLEAR INTERFACES TO WORK WITH EACH OTHER ===
If all the source code can fit into a 1M token context window, you are probably in good shape.

The following process takes time, but not nearly as much time as coding this with your own hands.

BONUS: You will end up with very good human-readable documentation.
BONUS: Major changes (like change to a different programming language) will become EASY.

1) Prompt foundation.md if you are starting a new project.
2) Look with your own eyes at README.md and SPECIFICATION.md; make sure they are good before proceeding.
3) Prompt startup.md
4) Look again with your own eyes at README.md and SPECIFICATION.md; make sure they are good before proceeding.
5) Prompt doc-consistency.md; go back to step 4 if any significant issues. Edit the documentation and/or tell the ai what changes to make to the documents.
6) Prompt code-consistency.md; go back to step 4 if any significant issues. Edit the documentation and/or tell the ai what changes to make to the documents.
7) Prompt test-fix-loop.md; interrupt this every 20 minutes or so, or if you see the agent drifting.
8) Look at ./tmp/report.md with your own eyes.
9) If tests pass and report has nothing important, you might be done.
10) If you need to adjust documentation, go back to step 4.
11) If documentation is good, prompt code-consistency.md; then go back to step 6.

When you find a bug that was not caught by testing:

12) edit and prompt bug-fix.md