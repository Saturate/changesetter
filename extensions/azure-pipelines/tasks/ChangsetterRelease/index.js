const tl = require("azure-pipelines-task-lib/task");
const https = require("https");
const { execSync } = require("child_process");
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

async function findMergedVersionPr() {
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  const urlPath = `${project}/_apis/git/repositories/${repoId}/pullrequests?searchCriteria.status=completed&searchCriteria.sourceRefName=refs/heads/changesetter/version-packages&$top=1&api-version=7.1`;

  const { data } = await apiRequest("GET", urlPath);
  if (data && data.value && data.value.length > 0) {
    return data.value[0];
  }
  return null;
}

async function findOpenVersionPr() {
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  const urlPath = `${project}/_apis/git/repositories/${repoId}/pullrequests?searchCriteria.status=active&searchCriteria.sourceRefName=refs/heads/changesetter/version-packages&$top=1&api-version=7.1`;

  const { data } = await apiRequest("GET", urlPath);
  if (data && data.value && data.value.length > 0) {
    return data.value[0];
  }
  return null;
}

async function createVersionPr(title, description) {
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  const urlPath = `${project}/_apis/git/repositories/${repoId}/pullrequests?api-version=7.1`;

  const { status, data } = await apiRequest("POST", urlPath, {
    sourceRefName: "refs/heads/changesetter/version-packages",
    targetRefName: "refs/heads/main",
    title,
    description,
    labels: [{ name: "changesetter:version" }],
  });

  if (status >= 200 && status < 300) {
    console.log(`Created version PR #${data.pullRequestId}`);
    return data;
  }
  console.log(`Failed to create PR: ${status} ${JSON.stringify(data)}`);
  return null;
}

async function updateVersionPr(prId, description) {
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  const urlPath = `${project}/_apis/git/repositories/${repoId}/pullrequests/${prId}?api-version=7.1`;

  const { status } = await apiRequest("PATCH", urlPath, { description });
  if (status >= 200 && status < 300) {
    console.log(`Updated version PR #${prId}`);
  } else {
    console.log(`Failed to update PR #${prId}: ${status}`);
  }
}

function exec(cmd) {
  return execSync(cmd, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
}

function hasChangesets() {
  try {
    const files = fs.readdirSync(".changeset");
    return files.some((f) => f.endsWith(".md") && f !== "README.md");
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

async function versionPrRelease(binDir, prTitle) {
  const bin = path.join(binDir, "changesetter");

  exec(`git config user.name "Azure Pipelines"`);
  exec(`git config user.email "azuredevops@users.noreply.dev.azure.com"`);

  if (!hasChangesets()) {
    const mergedPr = await findMergedVersionPr();
    if (mergedPr) {
      console.log("Detected merged version PR, running release for tagging...");
      // Read version from manifest since changesets were consumed by the version PR
      let version = "";
      try {
        const cargoToml = fs.readFileSync("Cargo.toml", "utf8");
        const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
        if (match) version = match[1];
      } catch {}
      if (!version) {
        try {
          const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
          version = pkg.version || "";
        } catch {}
      }

      if (!version) {
        console.log("Could not determine version after version PR merge");
        tl.setVariable("released", "false");
        return;
      }

      const tag = `v${version}`;
      exec(`git tag -a "${tag}" -m "Release ${version}" --cleanup=verbatim`);
      exec("git push --tags");

      tl.setVariable("released", "true");
      tl.setVariable("releases", JSON.stringify([{ name: path.basename(process.cwd()), version, tag }]));
      tl.setVariable("version", version);
      console.log(`Tagged ${tag}`);
    } else {
      console.log("No pending changesets and no merged version PR. Nothing to do.");
      tl.setVariable("released", "false");
    }
    return;
  }

  // Pending changesets exist: create/update version PR
  let status = "";
  try {
    status = exec(`"${bin}" status`);
  } catch {}

  exec("git checkout -B changesetter/version-packages");

  try {
    exec(`"${bin}" version --no-commit`);
  } catch {}

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
    if (pr) {
      console.log(`Created version PR #${pr.pullRequestId}`);
    }
  }

  tl.setVariable("released", "false");
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

run();
