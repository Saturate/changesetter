const tl = require("azure-pipelines-task-lib/task");
const https = require("https");
const { spawnSync } = require("child_process");
const path = require("path");
const { install } = require("../../shared/install");

async function postPrComment(content, passed, title) {
  const orgUrl = tl.getVariable("System.CollectionUri");
  const project = tl.getVariable("System.TeamProject");
  const repoId = tl.getVariable("Build.Repository.ID");
  const prId = tl.getVariable("System.PullRequest.PullRequestId");
  const token = tl.getVariable("System.AccessToken");

  if (!prId || !token) {
    console.log("Not a PR build or no access token; skipping comment.");
    return;
  }

  const url = new URL(
    `${orgUrl}${project}/_apis/git/repositories/${repoId}/pullRequests/${prId}/threads?api-version=7.1`
  );

  const status = title || (passed ? "Passed" : "Missing");
  const icon = passed ? "&#x2705;" : "&#x274C;";

  const body = JSON.stringify({
    comments: [
      {
        parentCommentId: 0,
        content: `${icon} **Changeset Check: ${status}**\n\n\`\`\`\n${content}\n\`\`\``,
        commentType: 1,
      },
    ],
    status: passed ? 4 : 1,
  });

  return new Promise((resolve, reject) => {
    const req = https.request(
      url,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${token}`,
          "Content-Length": Buffer.byteLength(body),
        },
      },
      (res) => {
        let data = "";
        res.on("data", (chunk) => (data += chunk));
        res.on("end", () => {
          if (res.statusCode >= 200 && res.statusCode < 300) {
            console.log("PR comment posted.");
            resolve();
          } else {
            console.log(`Failed to post PR comment: ${res.statusCode} ${data}`);
            resolve();
          }
        });
      }
    );
    req.on("error", (err) => {
      console.log(`Failed to post PR comment: ${err.message}`);
      resolve();
    });
    req.write(body);
    req.end();
  });
}

// The task result for the CLI's exit code and JSON report. A changeset that
// names an unknown package is a failed check, not a missing changeset.
function verdict(status, report) {
  if (status === 0) {
    const uncovered = (report && report.uncovered) || [];
    const note = uncovered.length ? ` (changed without a changeset: ${uncovered.join(", ")})` : "";
    return { passed: true, title: "Passed", message: `Changeset check passed${note}` };
  }
  if (!report) {
    return { passed: false, title: "Failed", message: "The changeset check couldn't finish; see the log." };
  }
  const unknown = report.unknown_packages || [];
  if (unknown.length) {
    const list = unknown.map((u) => `${u.file}: ${u.package}`).join("; ");
    return { passed: false, title: "Failed", message: `Changesets name packages that don't exist: ${list}` };
  }
  if (!(report.changesets || []).length) {
    return { passed: false, title: "Missing", message: "No changeset found. Run `changesetter add` to create one." };
  }
  return { passed: false, title: "Failed", message: "The changeset check failed; see the log." };
}

async function run() {
  try {
    const version = tl.getInput("version", false) || "latest";
    const base = tl.getInput("base", false);
    const comment = tl.getBoolInput("comment", false);

    const { binDir } = await install(version);
    tl.prependPath(binDir);

    let baseRef = base;
    if (!baseRef) {
      const prTarget = tl.getVariable("System.PullRequest.TargetBranch");
      if (prTarget) {
        baseRef = prTarget.replace("refs/heads/", "origin/");
      }
    }

    const args = ["check", "--output", "json"];
    if (baseRef) {
      args.push("--base", baseRef);
    }

    // An argument list, not a shell string: `base` is user input.
    const bin = path.join(binDir, "changesetter");
    const res = spawnSync(bin, args, { encoding: "utf8" });
    if (res.error) throw res.error;
    // stderr has the summary and the uncovered-package warning, which the
    // log should show whether or not the check passed.
    const log = (res.stderr || "").trim();
    if (log) console.log(log);

    let report = null;
    try {
      report = JSON.parse(res.stdout);
    } catch {}

    const { passed, title, message } = verdict(res.status, report);
    if (comment) {
      await postPrComment(log || message, passed, title);
    }
    if (passed) {
      tl.setResult(tl.TaskResult.Succeeded, message);
    } else {
      tl.setResult(tl.TaskResult.Failed, message);
    }
  } catch (err) {
    tl.setResult(tl.TaskResult.Failed, err.message);
  }
}

module.exports = { run, verdict };

if (require.main === module) {
  run();
}
