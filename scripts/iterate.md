# PromptHub Continuous Improvement Loop

## How to start

Dispatch the iteration-loop agent via the Agent tool:
- subagent_type: general-purpose
- Read `.claude/agents/iteration-loop.md` first, then pass its full content as the prompt, appending: "Start from round 1. The repository is at /home/mi/ssd/codes/claude-workspace/promptHub"

## What it does

Each round:
1. Creates a git worktree (`.worktrees/round-N`)
2. Runs 10 reviewer agents in parallel (read-only analysis)
3. Merges suggestions by priority tier
4. Runs implementer agent in the worktree
5. Validates: cargo build + test + clippy + new test required
6. Squash merges to master and pushes
7. Cleans up worktree
8. Starts round N+1

## How to stop

Ctrl+C at any time. The current round's worktree (if any) will be cleaned up on next start (Phase 0 cleanup).

## Failure handling

- Single round failure: skipped, loop continues
- 3 consecutive failures: loop halts, prints diagnostic

## Commit format

`<type>(round-N): iterative improvements`

No AI identifiers in any commits.
