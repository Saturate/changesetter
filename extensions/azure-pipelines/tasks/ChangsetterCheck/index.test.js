// Runs the ChangsetterCheck task against scratch git repos with tl, install
// and https stubbed. The CLI is real (CHANGESETTER_BIN, default
// target/debug/changesetter; run `cargo build` first). Usage: node index.test.js
const Module = require("module");
const https = require("https");
const { EventEmitter } = require("events");
const { execFileSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");

const REAL_CLI = path.resolve(process.env.CHANGESETTER_BIN || path.join(__dirname, "../../../../target/debug/changesetter"));
if (!fs.existsSync(REAL_CLI)) {
  console.error(`No changesetter binary at ${REAL_CLI}; run cargo build or set CHANGESETTER_BIN`);
  process.exit(1);
}

const state = { inputs: {}, vars: {}, result: null, comments: [] };
const tlStub = {
  getInput: (k) => state.inputs[k],
  getBoolInput: (k) => state.inputs[k] === "true",
  getVariable: (k) => state.vars[k],
  prependPath: () => {},
  setResult: (r, m) => { state.result = { r, m }; },
  TaskResult: { Succeeded: 0, Failed: 2 },
};
const origLoad = Module._load;
Module._load = function (request) {
  if (request === "azure-pipelines-task-lib/task") return tlStub;
  if (request.endsWith("shared/install")) return { install: async () => ({ binDir: path.dirname(REAL_CLI) }) };
  return origLoad.apply(this, arguments);
};
https.request = (url, opts, cb) => {
  const req = new EventEmitter();
  let body = "";
  req.write = (c) => { body += c; };
  req.end = () => {
    state.comments.push(JSON.parse(body).comments[0].content);
    const res = new EventEmitter();
    res.statusCode = 200;
    cb(res);
    res.emit("end");
  };
  return req;
};

const task = require("./index.js");
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "changesetter-check-task-"));
const git = (cwd, ...args) => execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...args], { cwd, encoding: "utf8" });

// Two npm packages on main, then a feature branch.
function repo(name) {
  const dir = path.join(tmpRoot, name);
  fs.mkdirSync(dir);
  git(dir, "init", "-q", "-b", "main");
  for (const pkg of ["web", "api"]) {
    fs.mkdirSync(path.join(dir, pkg));
    fs.writeFileSync(path.join(dir, pkg, "package.json"), JSON.stringify({ name: pkg, version: "1.0.0" }));
  }
  fs.mkdirSync(path.join(dir, ".changeset"));
  fs.writeFileSync(path.join(dir, ".changeset", "README.md"), "readme\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "base");
  git(dir, "checkout", "-q", "-b", "feature");
  return dir;
}

const changeset = (dir, file, frontmatter) =>
  fs.writeFileSync(path.join(dir, ".changeset", file), `---\n${frontmatter}\n---\n\nA change.\n`);

async function scenario(name, setup, check) {
  Object.assign(state, { inputs: { base: "main", comment: "true" }, vars: { "System.PullRequest.PullRequestId": "1", "System.AccessToken": "tok", "System.CollectionUri": "https://dev.azure.com/o/" }, result: null, comments: [] });
  console.log(`\n=== ${name}`);
  const dir = setup();
  process.chdir(dir);
  await task.run();
  check(dir);
  console.log(`PASS ${name}`);
}

(async () => {
  await scenario("changeset present: passes", () => {
    const dir = repo("pass");
    fs.writeFileSync(path.join(dir, "web", "index.js"), "x");
    changeset(dir, "a.md", "web: patch");
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", "change");
    return dir;
  }, () => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.result.m, "Changeset check passed");
    assert.match(state.comments[0], /Changeset Check: Passed/);
  });

  await scenario("no changeset: missing", () => {
    const dir = repo("missing");
    fs.writeFileSync(path.join(dir, "web", "index.js"), "x");
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", "change");
    return dir;
  }, () => {
    assert.strictEqual(state.result.r, 2);
    assert.match(state.result.m, /No changeset found/);
    assert.match(state.comments[0], /Changeset Check: Missing/);
  });

  await scenario("unknown package: failed, names the file and package", () => {
    const dir = repo("unknown");
    changeset(dir, "typo.md", "wbe: patch");
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", "typo");
    return dir;
  }, () => {
    assert.strictEqual(state.result.r, 2);
    assert.strictEqual(state.result.m, "Changesets name packages that don't exist: .changeset/typo.md: wbe");
    assert.match(state.comments[0], /Changeset Check: Failed/);
    assert.match(state.comments[0], /unknown package "wbe"/);
  });

  await scenario("uncovered package: passes and says which", () => {
    const dir = repo("uncovered");
    fs.writeFileSync(path.join(dir, "api", "index.js"), "x");
    changeset(dir, "a.md", "web: patch");
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", "change");
    return dir;
  }, () => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.result.m, "Changeset check passed (changed without a changeset: api)");
    assert.match(state.comments[0], /no changeset on this branch names it: api/);
  });

  await scenario("base input reaches the CLI as one argument, not through a shell", () => {
    const dir = repo("inject");
    changeset(dir, "a.md", "web: patch");
    git(dir, "add", "-A");
    git(dir, "commit", "-q", "-m", "change");
    state.inputs.base = "main; touch pwned";
    return dir;
  }, (dir) => {
    assert.ok(!fs.existsSync(path.join(dir, "pwned")), "base was run by a shell");
    assert.strictEqual(state.result.r, 2);
    assert.match(state.result.m, /couldn't finish/);
  });

  console.log("\nAll check task scenarios passed");
})().catch((e) => { console.error(e); process.exit(1); });
