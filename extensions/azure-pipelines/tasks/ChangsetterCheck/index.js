const tl = require("azure-pipelines-task-lib/task");
const https = require("https");
const { execSync } = require("child_process");
const path = require("path");
const { install } = require("../../shared/install");

async function postPrComment(content, passed) {
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

  const status = passed ? "Passed" : "Missing";
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

    const args = ["check"];
    if (baseRef) {
      args.push("--base", baseRef);
    }

    const bin = path.join(binDir, "changesetter");
    const cmd = `"${bin}" ${args.join(" ")}`;

    let output = "";
    let exitCode = 0;
    try {
      output = execSync(cmd, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
    } catch (err) {
      output = (err.stderr || "") + (err.stdout || "");
      exitCode = err.status || 1;
    }

    const passed = exitCode === 0;

    if (comment) {
      await postPrComment(output.trim() || (passed ? "Changeset found." : "No changeset found."), passed);
    }

    if (passed) {
      console.log(output);
      tl.setResult(tl.TaskResult.Succeeded, "Changeset check passed");
    } else {
      console.log(output);
      tl.setResult(tl.TaskResult.Failed, "No changeset found. Run `changesetter add` to create one.");
    }
  } catch (err) {
    tl.setResult(tl.TaskResult.Failed, err.message);
  }
}

run();
