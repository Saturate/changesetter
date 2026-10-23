---
changesetter-release-task: patch
changesetter-azure-pipelines: patch
---

Fix the Azure Pipelines release task treating every push after the first
release as a version PR merge and failing on the already existing tag. It now
releases only when the built commit is a completed version PR's merge commit,
skips the tag if it already exists on origin at that commit so a re-run works,
fails on a tag at a different commit (a version collision), and still opens a
version PR for changesets merged while the version PR was open. Changesets in
subdirectories now count, and a lone `.changeset/README.md` no longer opens an
empty version PR. A failed
Azure DevOps API call now fails the task instead of skipping the release. The
version-PR pipeline example in the Azure DevOps docs uses the same merge commit
lookup.
