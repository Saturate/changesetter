// Runs the PR comment script of actions/check/action.yml with stubbed
// github/context/core and the real CLI's JSON report, so the comment for each
// check outcome is tested without a pull request.
// Usage: node tests/actions/check-comment.test.js <changesetter binary>
const { execFileSync, spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");

const CLI = path.resolve(process.argv[2] || "target/debug/changesetter");
const action = path.join(__dirname, "../../actions/check/action.yml");
const script = execFileSync(
  "ruby",
  ["-ryaml", "-e", 'puts YAML.load_file(ARGV[0])["runs"]["steps"].find { |s| s["name"] == "Post PR comment" }["with"]["script"]', action],
  { encoding: "utf8" }
);
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
const comment = new AsyncFunction("require", "github", "context", "core", script);

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "changesetter-check-comment-"));
const git = (cwd, ...args) => execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...args], { cwd, encoding: "utf8" });

function repo(name, branch) {
  const dir = path.join(tmp, name);
  fs.mkdirSync(dir);
  git(dir, "init", "-q", "-b", "main");
  for (const pkg of ["web", "api"]) {
    fs.mkdirSync(path.join(dir, pkg));
    fs.writeFileSync(path.join(dir, pkg, "package.json"), JSON.stringify({ name: pkg, version: "1.0.0" }));
  }
  fs.mkdirSync(path.join(dir, ".changeset"));
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "base");
  git(dir, "checkout", "-q", "-b", "feature");
  branch(dir);
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "--allow-empty", "-m", "change");
  return dir;
}

// Runs the check the way the action's check step does, then the comment script.
async function run(dir) {
  const report = path.join(dir, "..", `${path.basename(dir)}.json`);
  const errors = path.join(dir, "..", `${path.basename(dir)}.err`);
  const res = spawnSync(CLI, ["check", "--base", "main", "--output", "json"], { cwd: dir, encoding: "utf8" });
  fs.writeFileSync(report, res.stdout);
  fs.writeFileSync(errors, res.stderr);
  Object.assign(process.env, { CHECK_REPORT: report, CHECK_ERRORS: errors, CHECK_EXIT_CODE: String(res.status) });
  const posted = [];
  const github = {
    paginate: async () => [],
    rest: { issues: { listComments: () => {}, createComment: async (c) => posted.push(c.body), updateComment: async (c) => posted.push(c.body) } },
  };
  const context = { repo: { owner: "o", repo: "r" }, issue: { number: 1 } };
  const core = { warning: (m) => { throw new Error(`unexpected warning: ${m}`); } };
  await comment(require, github, context, core);
  assert.strictEqual(posted.length, 1);
  return posted[0];
}

const write = (dir, file, text) => fs.writeFileSync(path.join(dir, file), text);

(async () => {
  let body = await run(repo("pass", (d) => { write(d, "web/x.js", "x"); write(d, ".changeset/a.md", "---\nweb: patch\n---\n\nFix\n"); }));
  assert.match(body, /### Changeset Check: Passed/);
  assert.doesNotMatch(body, /no changeset on this branch names/);
  console.log("ok   passed");

  body = await run(repo("missing", (d) => write(d, "web/x.js", "x")));
  assert.match(body, /### Changeset Check: Missing/);
  console.log("ok   missing");

  body = await run(repo("unknown", (d) => write(d, ".changeset/typo.md", "---\nwbe: patch\n---\n\nFix\n")));
  assert.match(body, /### Changeset Check: Failed/);
  assert.match(body, /`\.changeset\/typo\.md`: `wbe`/);
  assert.doesNotMatch(body, /No changeset found/);
  console.log("ok   unknown package");

  body = await run(repo("uncovered", (d) => { write(d, "api/x.js", "x"); write(d, ".changeset/a.md", "---\nweb: patch\n---\n\nFix\n"); }));
  assert.match(body, /### Changeset Check: Passed/);
  assert.match(body, /no changeset on this branch names them: `api`/);
  console.log("ok   uncovered");

  console.log("check comment tests passed");
})().catch((e) => { console.error(e); process.exit(1); });
