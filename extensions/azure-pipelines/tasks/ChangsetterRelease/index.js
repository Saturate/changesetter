const tl = require("azure-pipelines-task-lib/task");
const { execSync, execFile } = require("child_process");
const path = require("path");
// prepare.js copies shared/ into each task, since Azure Pipelines ships every
// task folder on its own.
const { install } = require("./shared/install");

function exec(cmd) {
  return execSync(cmd, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
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

  // --atomic: if the branch moved since checkout, the push is rejected, and
  // without it the tags still land, pointing at a commit on no branch.
  exec("git push --atomic --follow-tags");

  tl.setVariable("released", "true");
  tl.setVariable("releases", JSON.stringify(releases));
  tl.setVariable("version", releases[0].version);
  console.log(`Released: ${releases.map((r) => `${r.name}@${r.version}`).join(", ")}`);
}

// The CLI does the whole version-PR loop: it finds the version PR this
// commit merged, tags what it released, and creates or updates the next
// version PR. The task only maps pipeline variables to the environment the
// CLI reads and turns its JSON into output variables.
function runVersionPr(bin, prTitle) {
  const env = {
    ...process.env,
    SYSTEM_ACCESSTOKEN: tl.getVariable("System.AccessToken") || "",
    SYSTEM_COLLECTIONURI: tl.getVariable("System.CollectionUri") || "",
    SYSTEM_TEAMPROJECT: tl.getVariable("System.TeamProject") || "",
    BUILD_REPOSITORY_ID: tl.getVariable("Build.Repository.ID") || "",
    BUILD_REPOSITORY_NAME: tl.getVariable("Build.Repository.Name") || "",
    BUILD_SOURCEVERSION: tl.getVariable("Build.SourceVersion") || "",
  };
  const args = ["version-pr", "--provider", "azure", "--title", prTitle, "--output", "json"];
  // Async, not execFileSync: a blocked event loop would hold up anything
  // else in this process while the CLI runs.
  return new Promise((resolve, reject) => {
    execFile(bin, args, { env, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (stderr) process.stderr.write(stderr);
      if (err) {
        if (/unrecognized subcommand 'version-pr'/.test(String(stderr))) {
          reject(new Error("Version PR mode needs changesetter 0.9.0 or newer; set the task's version input to 0.9.0 or later"));
          return;
        }
        // The CLI's own error line comes first; git's trailing "error: failed
        // to push some refs" would hide the reason.
        const lines = String(stderr || "").trim().split("\n");
        const first = lines.find((l) => l.startsWith("error: ")) || err.message;
        reject(new Error(first.replace(/^error: /, "")));
        return;
      }
      // Thrown inside this callback it would be an uncaught exception, not a
      // failed task.
      try {
        resolve(JSON.parse(stdout));
      } catch (parseErr) {
        reject(new Error(`changesetter version-pr printed no JSON: ${parseErr.message}`));
      }
    });
  });
}

async function versionPrRelease(binDir, prTitle) {
  const bin = path.join(binDir, "changesetter");

  exec(`git config user.name "Azure Pipelines"`);
  exec(`git config user.email "azuredevops@users.noreply.dev.azure.com"`);

  tl.setVariable("released", "false");
  tl.setVariable("releases", "[]");

  const result = await runVersionPr(bin, prTitle);
  const tags = result.tags || [];
  if (tags.length > 0) {
    tl.setVariable("released", "true");
    tl.setVariable("releases", JSON.stringify(tags.map(({ name, version, tag }) => ({ name, version, tag }))));
    tl.setVariable("version", tags[0].version);
  }
  if (result.version_pr) {
    tl.setVariable("versionPrId", String(result.version_pr.id));
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

module.exports = { run, versionPrRelease };

if (require.main === module) {
  run();
}
