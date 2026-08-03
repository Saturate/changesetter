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

function fetch(url) {
  return new Promise((resolve, reject) => {
    const client = url.startsWith("https") ? https : http;
    client.get(url, { headers: { "User-Agent": "changesetter-azure-pipelines" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return fetch(res.headers.location).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
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
  console.log(`Installing changesetter v${version}`);

  const target = getTarget();
  const url = `https://github.com/saturate/changesetter/releases/download/v${version}/changesetter-${target}.tar.gz`;

  const tarball = await fetch(url);

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

module.exports = { install, getTarget };
