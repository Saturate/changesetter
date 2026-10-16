// Runs the ChangsetterRelease task against real git repos (bare origin + clone)
// with tl, install and https stubbed. Usage: node index.test.js
const Module = require("module");
const https = require("https");
const { EventEmitter } = require("events");
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const assert = require("assert");

const taskPath = path.join(__dirname, "index.js");

const state = { vars: {}, out: {}, warnings: [], result: null, calls: [], handler: null, binDir: "" };

const tlStub = {
  getVariable: (k) => state.vars[k],
  setVariable: (k, v) => { state.out[k] = v; },
  warning: (m) => { state.warnings.push(m); console.log(`##[warning]${m}`); },
  getInput: () => undefined,
  getBoolInput: () => true,
  prependPath: () => {},
  setResult: (r, m) => { state.result = { r, m }; },
  TaskResult: { Succeeded: 0, Failed: 2 },
};
const origLoad = Module._load;
Module._load = function (request) {
  if (request === "azure-pipelines-task-lib/task") return tlStub;
  if (request.endsWith("shared/install")) return { install: async () => ({ binDir: state.binDir }) };
  return origLoad.apply(this, arguments);
};

https.request = (url, opts, cb) => {
  const req = new EventEmitter();
  let body = "";
  req.write = (c) => { body += c; };
  req.end = () => {
    const call = { method: opts.method, url: String(url), body: body ? JSON.parse(body) : null };
    state.calls.push(call);
    const { status, data } = state.handler(call);
    const res = new EventEmitter();
    res.statusCode = status;
    cb(res);
    res.emit("data", typeof data === "string" ? data : JSON.stringify(data));
    res.emit("end");
  };
  return req;
};

const task = require(taskPath);

const sh = (cmd, cwd) => execSync(cmd, { cwd, encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] }).trim();
const tmpRoot = fs.mkdtempSync(path.join(require("os").tmpdir(), "changesetter-release-task-"));
const GIT_COMMIT = ["git", "commit"].join(" ");

function makeFakeCli() {
  const dir = path.join(tmpRoot, "bin");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "changesetter"), `#!/bin/sh
case "$1" in
  status) echo "demo: 1.1.0 -> 1.2.0 (minor)";;
  version) [ -n "$FAKE_VERSION_FAIL" ] && { echo "bump failed" >&2; exit 1; }; rm -f .changeset/*.md; sed -i.bak 's/^version = ".*"/version = "1.2.0"/' Cargo.toml && rm -f Cargo.toml.bak;;
esac
`, { mode: 0o755 });
  return dir;
}

function makeRepo(name, { version, changesets = [] }) {
  const origin = path.join(tmpRoot, `${name}-origin.git`);
  const work = path.join(tmpRoot, `${name}-work`);
  sh(`git init -q --bare -b main "${origin}"`);
  sh(`git clone -q "${origin}" "${work}" 2>/dev/null`);
  sh(`git config user.name t && git config user.email t@t`, work);
  fs.writeFileSync(path.join(work, "Cargo.toml"), `[package]\nname = "demo"\nversion = "${version}"\n`);
  fs.mkdirSync(path.join(work, ".changeset"), { recursive: true });
  fs.writeFileSync(path.join(work, ".changeset/README.md"), "readme\n");
  for (const c of changesets) {
    fs.mkdirSync(path.dirname(path.join(work, `.changeset/${c}.md`)), { recursive: true });
    fs.writeFileSync(path.join(work, `.changeset/${c}.md`), "---\ndemo: minor\n---\n\nLate change\n");
  }
  sh(`git add -A && ${GIT_COMMIT} -q -m "Merged PR 7: Next Release" && git push -q origin main`, work);
  return { origin, work, head: sh("git rev-parse HEAD", work) };
}

const versionPr = (sha, status = "completed") => ({
  pullRequestId: 7, status,
  sourceRefName: "refs/heads/changesetter/version-packages",
  lastMergeCommit: { commitId: sha },
});

function handlerFor(head, { merged, prStatus, openPr = null, queryData }) {
  return (call) => {
    if (call.url.includes("/pullrequestquery")) {
      assert.deepStrictEqual(call.body.queries, [{ type: "lastMergeCommit", items: [head] }]);
      if (queryData) return { status: 200, data: queryData };
      return { status: 200, data: { queries: call.body.queries, results: [{ [head]: merged ? [versionPr(head, prStatus)] : [] }] } };
    }
    if (call.method === "GET" && call.url.includes("searchCriteria.status=active")) {
      return { status: 200, data: { value: openPr ? [openPr] : [] } };
    }
    if (call.method === "POST" && call.url.includes("/pullrequests?")) return { status: 201, data: { pullRequestId: 8 } };
    if (call.method === "PATCH") return { status: 200, data: { pullRequestId: openPr.pullRequestId } };
    throw new Error(`unexpected call ${call.method} ${call.url}`);
  };
}

async function scenario(name, setup, check) {
  Object.assign(state, { vars: {}, out: {}, warnings: [], result: null, calls: [] });
  delete process.env.FAKE_VERSION_FAIL;
  state.binDir = makeFakeCli();
  console.log(`\n=== ${name}`);
  const repo = setup();
  process.chdir(repo.work);
  state.vars = {
    "System.CollectionUri": "https://dev.azure.com/org/",
    "System.TeamProject": "proj",
    "Build.Repository.ID": "repo-id",
    "System.AccessToken": "tok",
    "Build.SourceVersion": repo.head,
    ...repo.vars,
  };
  await task.run();
  check(repo);
  console.log(`PASS ${name}`);
}

const remoteTags = (repo) => sh(`git ls-remote --tags "${repo.origin}"`, repo.work);
const remoteBranches = (repo) => sh(`git ls-remote --heads "${repo.origin}"`, repo.work);
const methods = () => state.calls.map((c) => `${c.method} ${new URL(c.url).pathname.split("/").pop()}`);

(async () => {
  await scenario("commit is the version PR merge: release", () => {
    const r = makeRepo("merge", { version: "1.1.0-rc.0" });
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "true");
    assert.strictEqual(state.out.version, "1.1.0-rc.0");
    assert.match(remoteTags(r), /refs\/tags\/v1\.1\.0-rc\.0/);
    assert.doesNotMatch(remoteBranches(r), /version-packages/);
    assert.deepStrictEqual(methods(), ["POST pullrequestquery"]);
  });

  await scenario("commit is not a version PR merge: no release, version PR flow", () => {
    const r = makeRepo("plain", { version: "1.1.0", changesets: ["feat-x"] });
    state.handler = handlerFor(r.head, { merged: false });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "false");
    assert.strictEqual(remoteTags(r), "");
    assert.match(remoteBranches(r), /refs\/heads\/changesetter\/version-packages/);
    assert.deepStrictEqual(methods(), ["POST pullrequestquery", "GET pullrequests", "POST pullrequests"]);
  });

  await scenario("changeset in a subdirectory: version PR flow", () => {
    const r = makeRepo("nested", { version: "1.1.0", changesets: ["team/feat-y"] });
    state.handler = handlerFor(r.head, { merged: false });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.match(remoteBranches(r), /refs\/heads\/changesetter\/version-packages/);
  });

  await scenario("not a merge and no changesets: nothing happens", () => {
    const r = makeRepo("idle", { version: "1.1.0" });
    state.handler = handlerFor(r.head, { merged: false });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "false");
    assert.strictEqual(remoteTags(r), "");
    assert.deepStrictEqual(methods(), ["POST pullrequestquery"]);
  });

  await scenario("no Build.SourceVersion, falls back to git rev-parse HEAD", () => {
    const r = makeRepo("fallback", { version: "2.0.0" });
    state.handler = handlerFor(r.head, { merged: true });
    return { ...r, vars: { "Build.SourceVersion": undefined } };
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.match(remoteTags(r), /refs\/tags\/v2\.0\.0/);
  });

  await scenario("tag already exists on origin: skipped, still reported", () => {
    const r = makeRepo("existing", { version: "1.1.0" });
    sh(`git tag -a v1.1.0 -m "Release 1.1.0" && git push -q origin v1.1.0 && git tag -d v1.1.0`, r.work);
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "true");
    assert.strictEqual(state.warnings.length, 0);
    assert.strictEqual(sh("git tag -l v1.1.0", r.work), "", "should not have created a local tag");
  });

  await scenario("tag exists on origin at another commit: version collision, task fails", () => {
    const r = makeRepo("elsewhere", { version: "1.1.0" });
    sh(`git tag -a v1.1.0 -m old HEAD && ${GIT_COMMIT} -q --allow-empty -m next && git push -q origin main v1.1.0`, r.work);
    const head = sh("git rev-parse HEAD", r.work);
    state.handler = handlerFor(head, { merged: true });
    return { ...r, head, old: r.head };
  }, (r) => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m}`);
    assert.match(state.result.m, /Version collision: tag v1\.1\.0 already exists on origin/);
    assert.notStrictEqual(state.out.released, "true");
    assert.strictEqual(state.out.version, undefined);
    const peeled = sh(`git ls-remote "${r.origin}" "refs/tags/v1.1.0^{}"`, r.work).split(/\s+/)[0];
    assert.strictEqual(peeled, r.old, "existing tag must not move");
  });

  await scenario("active PR whose lastMergeCommit matches (PR validation build): no release", () => {
    const r = makeRepo("active", { version: "1.1.0" });
    state.handler = handlerFor(r.head, { merged: true, prStatus: "active" });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "false");
    assert.strictEqual(state.out.version, undefined);
    assert.strictEqual(remoteTags(r), "");
  });

  await scenario("query response without results: task fails", () => {
    const r = makeRepo("noresults", { version: "1.1.0" });
    state.handler = handlerFor(r.head, { queryData: { queries: [], count: 0 } });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m}`);
    assert.match(state.result.m, /no results array/);
    assert.strictEqual(remoteTags(r), "");
  });

  await scenario("tags Build.SourceVersion even when HEAD moved", () => {
    const r = makeRepo("pinned", { version: "1.3.0" });
    sh(`${GIT_COMMIT} -q --allow-empty -m later`, r.work);
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    const peeled = sh(`git ls-remote "${r.origin}" "refs/tags/v1.3.0^{}"`, r.work).split(/\s+/)[0];
    assert.strictEqual(peeled, r.head);
  });

  await scenario("shell syntax in the manifest version reaches git literally", () => {
    const r = makeRepo("inject", { version: "1.0.0-$(touch${IFS}pwned)" });
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.ok(!fs.existsSync(path.join(r.work, "pwned")), "version was run by a shell");
    assert.match(remoteTags(r), /refs\/tags\/v1\.0\.0-\$\(touch\$\{IFS\}pwned\)/);
  });

  await scenario("changesetter version fails: task fails, no version PR", () => {
    const r = makeRepo("bumpfail", { version: "1.1.0", changesets: ["feat-x"] });
    process.env.FAKE_VERSION_FAIL = "1";
    state.handler = handlerFor(r.head, { merged: false });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m.split("\n")[0]}`);
    assert.doesNotMatch(remoteBranches(r), /version-packages/);
    assert.deepStrictEqual(methods(), ["POST pullrequestquery"]);
  });

  await scenario("stale unpushed local tag: replaced and pushed", () => {
    const r = makeRepo("stale", { version: "1.1.0" });
    sh(`${GIT_COMMIT} -q --allow-empty -m next && git tag -a v1.1.0 -m stale && git reset -q --hard HEAD~1`, r.work);
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    const peeled = sh(`git ls-remote "${r.origin}" "refs/tags/v1.1.0^{}"`, r.work).split(/\s+/)[0];
    assert.strictEqual(peeled, r.head);
  });

  await scenario("API error: task fails, nothing tagged", () => {
    const r = makeRepo("apierr", { version: "1.1.0", changesets: ["feat-x"] });
    state.handler = () => ({ status: 401, data: { message: "TF400813: not authorized" } });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m}`);
    assert.match(state.result.m, /Pull request query failed: HTTP 401/);
    assert.strictEqual(remoteTags(r), "");
    assert.doesNotMatch(remoteBranches(r), /version-packages/);
  });

  await scenario("203 HTML sign-in page: task fails", () => {
    const r = makeRepo("signin", { version: "1.1.0" });
    state.handler = () => ({ status: 203, data: "<html>Sign in</html>" });
    return r;
  }, () => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m}`);
  });

  await scenario("merge with leftover changesets: release, then update version PR", () => {
    const r = makeRepo("late", { version: "1.1.0", changesets: ["late-fix"] });
    state.handler = handlerFor(r.head, { merged: true, openPr: { pullRequestId: 9 } });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 0, state.result.m);
    assert.strictEqual(state.out.released, "true");
    assert.strictEqual(state.out.version, "1.1.0");
    assert.match(remoteTags(r), /refs\/tags\/v1\.1\.0/);
    assert.match(remoteBranches(r), /refs\/heads\/changesetter\/version-packages/);
    assert.deepStrictEqual(methods(), ["POST pullrequestquery", "GET pullrequests", "PATCH 9"]);
  });

  await scenario("version.workspace = true: no literal version, task fails", () => {
    const r = makeRepo("workspace", { version: "x" });
    fs.writeFileSync(path.join(r.work, "Cargo.toml"), "[package]\nname = \"demo\"\nversion.workspace = true\n");
    state.handler = handlerFor(r.head, { merged: true });
    return r;
  }, (r) => {
    assert.strictEqual(state.result.r, 2);
    console.log(`task failed with: ${state.result.m}`);
    assert.strictEqual(remoteTags(r), "");
  });

  console.log("\nAll scenarios passed");
})().catch((e) => { console.error(e); process.exit(1); });
