const tl = require("azure-pipelines-task-lib/task");
const https = require("https");
const { execSync, execFileSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const { install } = require("../../shared/install");

function apiRequest(method, urlPath, body) {
  const orgUrl = tl.getVariable("System.CollectionUri");
  const token = tl.getVariable("System.AccessToken");
  const url = new URL(`${orgUrl}${urlPath}`);

  const payload = body ? JSON.stringify(body) : null;

  return new Promise((resolve, reject) => {
    const req = https.request(url, {
      method,
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
        ...(payload ? { "Content-Length": Buffer.byteLength(payload) } : {}),
      },
    }, (res) => {
      let data = "";
      res.on("data", (chunk) => (data += chunk));
      res.on("end", () => {
        try {
          resolve({ status: res.statusCode, data: data ? JSON.parse(data) : null });
        } catch {
          resolve({ status: res.statusCode, data });
        }
      });
    });
    req.on("error", reject);
    if (payload) req.write(payload);
    req.end();
  });
}

const VERSION_BRANCH_REF = "refs/heads/changesetter/version-packages";

function repoApiPath(suffix) {
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  return `${project}/_apis/git/repositories/${repoId}/${suffix}`;
}

// Azure DevOps answers a missing or under-scoped token with a 203 HTML sign-in
// page, so a 2xx status alone does not mean the call worked.
function assertJsonOk(what, { status, data }) {
  if (status < 200 || status >= 300 || !data || typeof data !== "object") {
    const detail = typeof data === "string" ? data.slice(0, 200) : JSON.stringify(data);
    throw new Error(`${what} failed: HTTP ${status} ${detail}`);
  }
  return data;
}

function currentCommit() {
  const sourceVersion = tl.getVariable("Build.SourceVersion");
  if (sourceVersion) return sourceVersion.trim().toLowerCase();
  return exec("git rev-parse HEAD").trim().toLowerCase();
}

// Looks the commit up by merge commit instead of listing completed version
// PRs: the newest completed PR stays "merged" forever, so every later push
// would look like a version PR merge. A failed lookup throws, because
// treating it as "not a version PR" would skip the release with a green build.
async function findMergedVersionPr(commitId) {
  const response = await apiRequest("POST", repoApiPath("pullrequestquery?api-version=7.1"), {
    queries: [{ type: "lastMergeCommit", items: [commitId] }],
  });
  const data = assertJsonOk("Pull request query", response);
  if (!Array.isArray(data.results)) {
    throw new Error(`Pull request query returned no results array: ${JSON.stringify(data)}`);
  }

  const prs = data.results.flatMap((byCommit) =>
    Object.entries(byCommit || {})
      .filter(([sha]) => sha.toLowerCase() === commitId)
      .flatMap(([, list]) => list || [])
  );

  return (
    prs.find(
      (pr) =>
        pr.sourceRefName === VERSION_BRANCH_REF &&
        pr.status === "completed" &&
        pr.lastMergeCommit &&
        String(pr.lastMergeCommit.commitId).toLowerCase() === commitId
    ) || null
  );
}

async function findOpenVersionPr() {
  const urlPath = repoApiPath(
    `pullrequests?searchCriteria.status=active&searchCriteria.sourceRefName=${VERSION_BRANCH_REF}&$top=1&api-version=7.1`
  );

  // Throws on failure so a broken lookup does not open a duplicate version PR.
  const data = assertJsonOk("Open version PR lookup", await apiRequest("GET", urlPath));
  if (data.value && data.value.length > 0) {
    return data.value[0];
  }
  return null;
}

async function createVersionPr(title, description) {
  const response = await apiRequest("POST", repoApiPath("pullrequests?api-version=7.1"), {
    sourceRefName: VERSION_BRANCH_REF,
    targetRefName: "refs/heads/main",
    title,
    description,
    labels: [{ name: "changesetter:version" }],
  });
  return assertJsonOk("Creating version PR", response);
}

async function updateVersionPr(prId, description) {
  const response = await apiRequest("PATCH", repoApiPath(`pullrequests/${prId}?api-version=7.1`), { description });
  assertJsonOk(`Updating version PR #${prId}`, response);
}

function exec(cmd) {
  return execSync(cmd, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
}

// No shell: the version comes from Cargo.toml or package.json, and a value
// like `$(...)` must reach git as a literal argument.
function git(...args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
}

function hasChangesets() {
  try {
    // Changesets can sit in subdirectories; README.md is never one.
    const entries = fs.readdirSync(".changeset", { recursive: true, withFileTypes: true });
    return entries.some((e) => e.isFile() && e.name.endsWith(".md") && e.name !== "README.md");
  } catch {
    return false;
  }
}

async function directRelease(binDir) {
  const bin = path.join(binDir, "changesetter");

  exec(`git config user.name "Azure Pipelines"`);
  exec(`git config user.email "azuredevops@users.noreply.dev.azure.com"`);

  let output;
  try {
    output = exec(`"${bin}" release --output json`);
  } catch (err) {
    console.log("changesetter release exited non-zero (likely no changesets)");
    tl.setVariable("released", "false");
    tl.setVariable("releases", "[]");
    return;
  }

  let parsed;
  try {
    parsed = JSON.parse(output);
  } catch {
    console.log("No JSON output from release (nothing to release)");
    tl.setVariable("released", "false");
    tl.setVariable("releases", "[]");
    return;
  }

  const releases = parsed.releases || [];
  if (releases.length === 0) {
    tl.setVariable("released", "false");
    tl.setVariable("releases", "[]");
    return;
  }

  exec("git push --follow-tags");

  tl.setVariable("released", "true");
  tl.setVariable("releases", JSON.stringify(releases));
  tl.setVariable("version", releases[0].version);
  console.log(`Released: ${releases.map((r) => `${r.name}@${r.version}`).join(", ")}`);
}

// The version PR already bumped the manifest, so it holds the version being
// released. Not `changesetter status`: changesets merged while the version PR
// was open would make it report another package's version. Only a literal
// `version = "..."` counts, so `version.workspace = true` is not taken as one.
function readManifestVersion() {
  try {
    const cargoToml = fs.readFileSync("Cargo.toml", "utf8");
    const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
    if (match) return match[1];
  } catch {}
  try {
    const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
    if (typeof pkg.version === "string" && pkg.version) return pkg.version;
  } catch {}
  return "";
}

// Returns the commit an existing remote tag points to, or "" if it is absent.
// Asks origin directly: a self-hosted agent can keep a local tag from an
// earlier run that never got pushed.
function remoteTagCommit(tag) {
  const out = git("ls-remote", "--tags", "origin", `refs/tags/${tag}`, `refs/tags/${tag}^{}`).trim();
  if (!out) return "";
  const lines = out.split("\n").map((l) => l.split(/\s+/));
  const peeled = lines.find(([, ref]) => ref.endsWith("^{}"));
  return (peeled || lines[0])[0].toLowerCase();
}

function tagRelease(version, commitId) {
  const tag = `v${version}`;
  const existing = remoteTagCommit(tag);
  if (existing && existing !== commitId) {
    // Another commit already shipped as this version. Reporting a release
    // here would let downstream steps publish this commit under that version.
    throw new Error(
      `Version collision: tag ${tag} already exists on origin at ${existing}, not at ${commitId}. ` +
        "Bump the version in a new version PR, or move the tag if it is wrong."
    );
  }
  if (existing) {
    console.log(`Tag ${tag} already exists on origin at ${commitId}, skipping tag creation`);
  } else {
    // -f replaces a stale local tag that a previous run created but never pushed.
    git("tag", "-a", "-f", tag, "-m", `Release ${version}`, "--cleanup=verbatim", commitId);
    git("push", "origin", `refs/tags/${tag}`);
    console.log(`Tagged ${tag}`);
  }

  tl.setVariable("released", "true");
  tl.setVariable("releases", JSON.stringify([{ name: path.basename(process.cwd()), version, tag }]));
  tl.setVariable("version", version);
}

async function versionPrRelease(binDir, prTitle) {
  const bin = path.join(binDir, "changesetter");

  exec(`git config user.name "Azure Pipelines"`);
  exec(`git config user.email "azuredevops@users.noreply.dev.azure.com"`);

  tl.setVariable("released", "false");
  tl.setVariable("releases", "[]");

  const commitId = currentCommit();
  const mergedPr = await findMergedVersionPr(commitId);
  if (mergedPr) {
    console.log(`Commit ${commitId} is the merge of version PR #${mergedPr.pullRequestId}, releasing`);
    const version = readManifestVersion();
    if (!version) {
      throw new Error("Could not determine the released version from Cargo.toml or package.json");
    }
    tagRelease(version, commitId);
  } else {
    console.log(`Commit ${commitId} is not a version PR merge`);
  }

  // Also after a release: changesets merged while the version PR was open are
  // still in the tree and need a fresh version PR.
  if (!hasChangesets()) {
    console.log("No pending changesets, no version PR to update.");
    return;
  }

  // Pending changesets exist: create/update version PR
  let status = "";
  try {
    status = exec(`"${bin}" status`);
  } catch {}

  exec("git checkout -B changesetter/version-packages");

  // A failed bump can leave a half-edited tree, which must not become a version PR.
  exec(`"${bin}" version --no-commit`);

  exec("git add -A");
  exec('git commit -m "chore: version packages" --allow-empty');
  exec("git push origin changesetter/version-packages --force");
  exec("git checkout -");

  const description = `This PR was opened by changesetter. Merging it will release the following packages.\n\n\`\`\`\n${status}\n\`\`\``;

  const existingPr = await findOpenVersionPr();
  if (existingPr) {
    await updateVersionPr(existingPr.pullRequestId, description);
    console.log(`Updated existing version PR #${existingPr.pullRequestId}`);
  } else {
    const pr = await createVersionPr(prTitle, description);
    console.log(`Created version PR #${pr.pullRequestId}`);
  }
}

async function run() {
  try {
    const version = tl.getInput("version", false) || "latest";
    const versionPr = tl.getBoolInput("versionPr", false);
    const versionPrTitle = tl.getInput("versionPrTitle", false) || "Next Release";

    const { binDir } = await install(version);
    tl.prependPath(binDir);

    if (versionPr) {
      await versionPrRelease(binDir, versionPrTitle);
    } else {
      await directRelease(binDir);
    }

    tl.setResult(tl.TaskResult.Succeeded, "Done");
  } catch (err) {
    tl.setResult(tl.TaskResult.Failed, err.message);
  }
}

module.exports = { run, versionPrRelease, findMergedVersionPr, readManifestVersion };

if (require.main === module) {
  run();
}
