Please read aitc/DESIGN.md and aitc/PHILOSOPHY.md ; this is the first time ./aitc/ has been used to completion on any project. let's see if we can make it work. be ready to stop, fix, and restart. the goal is to perfect ./aitc/ not to build this project. you can start by running aitc/scripts/software-construction.py unless it is already running; either way, babysit it and be ready to make adjustments

Btw, if diagnose is triggered, it might be normal, but more likely it means there is a problem with ./aitc/ or the PREFER-LO-AI.flag is giving prompts to spark that are too difficult for it.

A good sign is if released jars are being created and they pass tests.

Over-decomposition is a bad sign that ./atic/ is screwing up. But we can forgive a little over-decomposition as long as leaf projects exist and they are creating released binaries that pass tests.
