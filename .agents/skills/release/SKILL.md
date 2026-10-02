---
name: release
description: Release a new version of a versioned project by choosing the bump level, tagging it, creating the hosted release, and running the repository's publish path. Use when the user asks to release, publish, cut, or tag a version, or asks whether a completed change should become one. Not for committing changes, creating branches, or publishing an artifact outside a versioned release.
argument-hint: "[patch|minor|major] [context]"
allowed-tools:
  - Bash(git status:*)
  - Bash(git log:*)
  - Bash(git diff:*)
  - Bash(git show:*)
  - Bash(git ls-remote:*)
  - Read
---

Turn the current state of a repository into one published version, or report why it should not be one yet.

## Gate

Tagging, pushing and publishing expose a version to consumers. An already authorized release continues with that exact version and batch; otherwise the first pass ends with a proposal, never with a release action. A consumed tag is not moved.

- Release only when the user asked for this release, or answered a proposal about this exact version. A request to commit, push, merge, or finish a change is not a request to release it.
- Confirm the proposed version does not already exist at the destination before proposing it; never republish a version that exists.
- Do not fold publishing into a commit, push, or release-creation step the user authorized separately.
- When the repository publishes from a tag or a release event, the push carrying that ref is the release action. It needs the same direction as publishing, and it is the point of no return, so push that ref only as the last step of an authorized release.
- When a completed change is one the repository habitually releases, offer the release in one line with the proposed level. Offer, then wait; do not open a release because a change is finished.

## Establish the repository's release contract before proposing

The repository owns how its version becomes a release. Read its instructions, [release documentation](../../../docs/setup.md#release), CI workflows, and any release script, then state what completion means here.

- Do not assume a toolchain: the version may be bumped by hand, by a script, by a workflow on a tag or release event, or by a release automation tool.
- Identify what starts the publish path, because it decides the agent's last action: when the pipeline reacts to a pushed tag or release, the agent stops at the push and the automation publishes; otherwise the agent runs the remaining steps.
- Enumerate the steps this repository actually has, in order: version file, bump commit, tag, hosted release, publish job, and any downstream copy of the version such as a packaging manifest, lockfile, or installer formula.
- Note which step needs the user rather than the agent, for example a publish credential or a manual approval.

## Choose the level

Derive the level from the range that this release would include, not from the request: read the commits and diff since the last released version, and compare the user-visible contract before and after.

- Major: a caller or user of the released interface must change something, or a documented guarantee is removed.
- Minor: a new capability, or a behavior change users must act on, including a widened or narrowed scope of what the artifact blocks, allows, or handles.
- Patch: fixes, internal change, dependency or documentation updates that leave user-visible behavior unchanged.
- The range starts at the last version that was both tagged and published. A prepared version whose commit and tag were never pushed, and that nobody consumed, is not a released version: rewrite its release commit into the next intended version instead of releasing it, re-derive the level from the whole range, leave no file, note, or commit subject that names the abandoned number, and check the remote refs immediately before rewriting.
- Follow the repository's documented policy when it has one, including how it treats pre-1.0 versions; otherwise apply the rules above.
- When the range mixes levels, the highest level wins. When it is ambiguous between two levels, say so and recommend one.

## Propose

When authorization is missing, send one proposal: the version, the level with the evidence that decided it, the steps the repository will run in order, the destination of the published artifact, and any step that needs the user. Wait for direction on that version; reuse an existing authorization that still covers it and the publication batch.

## Release notes

Write for people installing or using the version. Select changes that affect observable behavior, compatibility, supported environments, installation, or required user action. Describe the consequence; include implementation details only when they explain it. Routine development work and validation results belong in the operator's completion report. If the release contains only internal maintenance, say so briefly without inventing a user-visible change.

Before publication, compare the notes with the release range and owning public contract. Remove work summaries with no relevant user consequence, and preserve changed requirements, limits, and migration steps, including whether an action is required and when it must occur.

## Execute

After direction for that exact version, follow the repository's own path. Keep the version bump and its Release notes in a separate commit with subject `chore: release vX.Y.Z`, substituting the actual version. This separation permits folding an unconsumed preparation without rewriting a feature commit that may already be pushed. When the repository publishes from a tag or a release event, write the version into its owning file, create that release commit and the tag, then push the commit and tag and leave publishing and the release record to the automation. Otherwise run the remaining steps in the repository's order, with the hosted release before the publish step.

- Use the repository's tag and release conventions; when it has none, use `v<version>` for the tag.
- Order the steps so a failure leaves nothing published.
- Verify each step before the next: the owning file holds the new version, the tag names that version, and the run that the push started appears. Then follow that run to a terminal state within a bounded wait, check its result, and confirm at the destination that the artifact exists, that its contents match the repository's include list, and that the attestation or provenance the destination offers is present.

## When it fails

- Query the destination before any retry: when the version is absent there, the publish can be retried after the cause is fixed; when it is present, the release exists and only the next version can correct it.
- A failed automation run takes the same check before a re-run, because a re-run publishes the same version again. Re-run the same ref only while the destination does not hold that version, unless the repository documents a partial re-run, such as rerunning only the hosted-release job after the publish step succeeded; fix the cause before re-running rather than re-running repeatedly.
- A tag that someone consumed is not moved. Correct a wrong or incomplete release by publishing the next version.
- Report a downstream copy that the release did not update as an open gap, with the steps it needs.

## Output

Return the version and level with the deciding evidence, the tag, the hosted release link, the publish result, the verification performed, and every step left open.
