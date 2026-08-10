---
name: performance-engineering
description: Measure and improve Rust performance with explicit budgets, profiling, benchmarks, algorithm analysis, and regression evidence. Use when latency, throughput, memory, CPU, allocation, or contention is a stated requirement.
compatibility: Codex, OpenCode, Claude Code, and Qwen Code
---

# Performance Engineering

Do not optimize from intuition alone. Define the metric, collect a baseline, change one meaningful variable, and measure again.

1. Write the performance requirement and workload shape.
2. Check algorithmic complexity and avoid avoidable copies or allocations.
3. Build a representative benchmark or load test.
4. Profile the relevant bottleneck.
5. Make the smallest clear change.
6. Repeat the measurement under the same conditions.
7. Add a regression threshold when the metric is stable enough to automate.

Never trade correctness, security, or maintainability for an unmeasured speedup.
