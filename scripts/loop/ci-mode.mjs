#!/usr/bin/env node
/**
 * scripts/loop/ci-mode.mjs — is GitHub Actions actually able to run for this repo RIGHT NOW?
 * Ported from the Drivia harness. Same discriminator, Yap's repo, and the same deliberate bias:
 * UNKNOWN RESOLVES TO LOCAL. On wilsonguenther-dev/wilson-voice the newest workflow run is weeks
 * old and Actions is disabled account-wide by a spending limit, so `local` is the expected answer
 * and `github` is the exception that has to be earned by evidence.
 *
 *     node scripts/loop/ci-mode.mjs           → prints "github" or "local"
 *     node scripts/loop/ci-mode.mjs --why     → prints the mode plus the evidence line
 *
 * WHY THIS EXISTS. On 2026-09-05 every workflow run on the sibling repo began failing in
 * ~2 seconds. The runs were created, the jobs were created, and then GitHub refused to place them
 * on a runner — the account's Actions spending limit was reached. The annotation on every job is:
 *
 *   "The job was not started because recent account payments have failed or your spending limit
 *    needs to be increased. Please check the 'Billing & plans' section in your settings"
 *
 * That is NOT "zero jobs". A run in this state has jobs with conclusion "failure", no runner_name,
 * and an EMPTY steps[] — a job that never started. So the discriminator is not "did a run exist"
 * and not "were jobs created": it is "did at least one job actually START". Anything that reads
 * only run.conclusion, or only jobs.total_count, reports a healthy CI on a dead one — which is the
 * exact false-green this loop's telemetry keeps paying for.
 *
 * CONTRACT
 *   github  the newest pull_request-triggered run created in the last 24 h reached at least one
 *           job that started (has steps, or was assigned a runner), or is still executing.
 *   local   it did not — or there is no such run to measure, or gh itself failed. UNKNOWN RESOLVES
 *           TO LOCAL ON PURPOSE: the local gate is strictly stronger than the CI gate (the agent
 *           runs tsc, vitest, the vite build, the sidecar build, both cargo test suites and clippy
 *           itself and pastes eight bare exit codes), so guessing "local" costs time, while guessing
 *           "github" would let a PR merge on a check that can never turn green.
 *
 * No dependencies. Shells out to the already-authenticated `gh` CLI.
 */
import { execFileSync } from 'node:child_process'

const REPO = process.env.LOOP_REPO || 'wilsonguenther-dev/wilson-voice'
const WINDOW_HOURS = 24
const WHY = process.argv.includes('--why')

/** A job "started" if GitHub ever handed it to a runner. A billing-blocked job never does. */
const jobStarted = (job) =>
  (Array.isArray(job.steps) && job.steps.length > 0) ||
  (typeof job.runner_name === 'string' && job.runner_name.length > 0) ||
  job.status === 'in_progress'

const gh = (endpoint) =>
  JSON.parse(execFileSync('gh', ['api', '-H', 'Accept: application/vnd.github+json', endpoint], {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  }))

let mode = 'local'
let why = 'no evidence gathered'
try {
  const runs = gh(`repos/${REPO}/actions/runs?event=pull_request&per_page=10`).workflow_runs || []
  const cutoff = Date.now() - WINDOW_HOURS * 3600 * 1000
  const recent = runs
    .filter((r) => Date.parse(r.created_at) >= cutoff)
    .sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))

  if (recent.length === 0) {
    why = `no pull_request-triggered run on ${REPO} in the last ${WINDOW_HOURS}h — nothing to measure, so the local gate stands in`
  } else {
    // The newest run decides. If it is inconclusive (still queued, no jobs yet), fall through to
    // the next two — a single fluke run must not condemn a healthy CI, or resurrect a dead one.
    for (const run of recent.slice(0, 3)) {
      const jobs = gh(`repos/${REPO}/actions/runs/${run.id}/jobs?per_page=100`).jobs || []
      const started = jobs.filter(jobStarted)
      if (started.length > 0) {
        mode = 'github'
        why = `run ${run.id} (${run.created_at}) placed ${started.length}/${jobs.length} job(s) on a runner`
        break
      }
      const blocked = jobs.find((j) => j.conclusion === 'failure' && (!j.steps || j.steps.length === 0))
      why = blocked
        ? `run ${run.id} (${run.created_at}): ${jobs.length} job(s) created, 0 started — jobs failed with no steps and no runner (Actions cannot place work; check the account spending limit)`
        : `run ${run.id} (${run.created_at}): ${jobs.length} job(s), none started`
      if (blocked) break
    }
  }
} catch (err) {
  why = `gh query failed (${String(err && err.message).split('\n')[0]}) — resolving UNKNOWN to local`
  mode = 'local'
}

process.stdout.write(WHY ? `${mode}\n${why}\n` : `${mode}\n`)
