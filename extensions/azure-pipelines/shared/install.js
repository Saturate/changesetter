const crypto = require("crypto");
const https = require("https");
const http = require("http");
const fs = require("fs");
const path = require("path");
const os = require("os");
const { execSync } = require("child_process");

function getTarget() {
  const platform = os.platform();
  const arch = os.arch();

  let target;
  if (platform === "linux" && arch === "x64") target = "x86_64-unknown-linux-gnu";
  else if (platform === "linux" && arch === "arm64") target = "aarch64-unknown-linux-gnu";
  else if (platform === "darwin" && arch === "x64") target = "x86_64-apple-darwin";
  else if (platform === "darwin" && arch === "arm64") target = "aarch64-apple-darwin";
  else throw new Error(`Unsupported platform: ${platform}-${arch}`);

  return target;
}

class HttpError extends Error {
  constructor(status, url) {
    super(`HTTP ${status} for ${url}`);
    this.status = status;
  }
}

function fetch(url) {
  return new Promise((resolve, reject) => {
    const client = url.startsWith("https") ? https : http;
    client.get(url, { headers: { "User-Agent": "changesetter-azure-pipelines" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return fetch(res.headers.location).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new HttpError(res.statusCode, url));
      }
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => resolve(Buffer.concat(chunks)));
      res.on("error", reject);
    }).on("error", reject);
  });
}

async function resolveLatestVersion() {
  const data = await fetch("https://api.github.com/repos/saturate/changesetter/releases/latest");
  const release = JSON.parse(data.toString());
  const tag = release.tag_name || "";
  return tag.replace(/.*v/, "");
}

async function install(version) {
  if (!version || version === "latest") {
    version = await resolveLatestVersion();
  }
  version = version.replace(/^changesetter@/, "").replace(/^v/, "");
  console.log(`Installing changesetter v${version}`);

  const target = getTarget();
  // CLI releases are tagged changesetter@v<version>; plain v<version> tags
  // are the GitHub Action releases and have no binaries.
  const base = `https://github.com/saturate/changesetter/releases/download/changesetter%40v${version}`;
  const archive = `changesetter-${target}.tar.gz`;

  const tarball = await fetch(`${base}/${archive}`);
  await verifyChecksum(base, archive, tarball);

  const tmpDir = path.join(os.tmpdir(), `changesetter-${Date.now()}`);
  fs.mkdirSync(tmpDir, { recursive: true });

  const tarPath = path.join(tmpDir, "changesetter.tar.gz");
  fs.writeFileSync(tarPath, tarball);

  execSync(`tar xzf "${tarPath}" -C "${tmpDir}"`);

  const binPath = path.join(tmpDir, "changesetter");
  if (!fs.existsSync(binPath)) {
    throw new Error(`Binary not found at ${binPath} after extraction`);
  }

  fs.chmodSync(binPath, 0o755);
  return { binPath, binDir: tmpDir, version };
}

// Releases from 0.9.0 on publish SHA256SUMS. An older release has none, so
// its download can only be trusted as far as the HTTPS connection; a
// mismatch against a published sum is always fatal.
async function verifyChecksum(base, archive, data) {
  let sums;
  try {
    sums = (await fetch(`${base}/SHA256SUMS`)).toString();
  } catch (err) {
    if (err instanceof HttpError && err.status === 404) {
      console.log(`No SHA256SUMS published for this release; ${archive} is not checksum-verified`);
      return;
    }
    throw err;
  }
  const expected = sums
    .split("\n")
    .map((line) => line.trim().split(/\s+\*?/))
    .find(([, name]) => name === archive);
  if (!expected) throw new Error(`SHA256SUMS has no entry for ${archive}`);
  const actual = crypto.createHash("sha256").update(data).digest("hex");
  if (actual !== expected[0].toLowerCase()) {
    throw new Error(`Checksum mismatch for ${archive}: expected ${expected[0]}, got ${actual}`);
  }
  console.log(`Verified ${archive} against SHA256SUMS`);
}

module.exports = { install, getTarget, verifyChecksum };
